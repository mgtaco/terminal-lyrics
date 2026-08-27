//! Finding lyrics for a track.
//!
//! Order: local `--lrc-dir` file, then cache (positive *and* negative), then
//! each network provider in turn until one answers with word timings.
//! Everything is async and runs off the UI task, so a slow network never
//! freezes the display.
//!
//! The providers are deliberately ordered by what they are *good at* rather
//! than by how often they hit:
//!
//! * **AMLL** is a static git repo that will outlive every hosted service, it
//!   is syllable-level, and being keyed by the Spotify ID it costs one request
//!   and no matching guesswork. When it hits, nothing else needs asking.
//! * **LyricsPlus** is Apple's own TTML — syllable-level too, and the dialect
//!   the renderer is already tuned for.
//! * **lrcmux** is the reach: five upstreams behind one API, so it degrades
//!   rather than dies.
//! * **LRCLIB** last, for the line-level answer that is better than nothing.
//!
//! That ordering is a prediction, though, and [`first_hit`] does not trust it
//! blindly: a provider that answers line-level is held as a fallback rather
//! than believed, and the chain keeps going. Only word timings stop it.
//!
//! Which of them run, and in what order, is [`Provider`] — a config list, not
//! a flag each, so a self-hoster can drop one by deleting a word.

pub mod amll;
pub mod cache;
pub mod lrclib;
pub mod lrcmux;
pub mod lyricsplus;
pub mod normalize;
pub mod ttml;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use anyhow::Result;
use serde::Deserialize;

use self::cache::Cache;
use self::lrclib::LrcLib;
use crate::lrc::{self, Lyrics};
use crate::player::Track;

/// Beyond this the "match" is a different edit of the song and the timings are
/// wrong throughout — worse than showing nothing. Every provider that can check
/// a duration applies it, so one of them cannot quietly be laxer than the rest.
pub const MAX_DURATION_DELTA: f64 = 5.0;

/// The default base URLs. Both services are small and community-run, and both
/// document self-hosting, so both are overridable in the config.
pub const LYRICSPLUS_URL: &str = "https://lyricsplus.prjktla.my.id";
pub const LRCMUX_URL: &str = "https://api.lrcmux.dev";

/// Where a set of lyrics came from — shown by `lyrics status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    LocalFile(PathBuf),
    Cache,
    LrcLib { id: i64 },
    /// The AMLL TTML database — syllable-timed, keyed by Spotify ID.
    Amll,
    /// LyricsPlus, serving Apple Music's syllable-timed TTML.
    LyricsPlus,
    /// lrcmux, carrying the id of whichever upstream actually answered —
    /// worth showing, because "the lyrics are junk" and "musixmatch is
    /// returning junk" are different problems.
    LrcMux { provider: String },
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::LocalFile(p) => write!(f, "file {}", p.display()),
            Source::Cache => write!(f, "cache"),
            Source::LrcLib { id } => write!(f, "lrclib #{id}"),
            Source::Amll => write!(f, "amll"),
            Source::LyricsPlus => write!(f, "lyricsplus"),
            Source::LrcMux { provider } if provider.is_empty() => write!(f, "lrcmux"),
            Source::LrcMux { provider } => write!(f, "lrcmux/{provider}"),
        }
    }
}

impl Source {
    /// Short form for the one-line status strip, where a full path would crowd
    /// out everything else.
    pub fn short(&self) -> String {
        match self {
            Source::LocalFile(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string()),
            other => other.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Found {
    pub lyrics: Lyrics,
    pub source: Source,
    /// False when only unsynced plain text was available.
    pub synced: bool,
    /// The document exactly as received. The cache stores *this*, not
    /// `lyrics.plain_text()` — round-tripping through plain text would drop
    /// every timestamp and silently unsync the lyrics on the next hit.
    pub raw: String,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Found(Box<Found>),
    /// The track is known to have no lyrics (or is instrumental).
    Missing,
}

/// `Artist - Title.lrc` next to the user's own files, checked before the network.
pub fn local_lookup(dir: &Path, track: &Track) -> Option<Found> {
    let stem = if track.artist.is_empty() {
        track.title.clone()
    } else {
        format!("{} - {}", track.artist, track.title)
    };
    for ext in ["lrc", "txt"] {
        let path = dir.join(format!("{stem}.{ext}"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let parsed = lrc::parse(&text);
        if parsed.is_empty() {
            continue;
        }
        let synced = parsed.lines.iter().any(|l| l.start > 0.0);
        return Some(Found {
            lyrics: parsed,
            source: Source::LocalFile(path),
            synced,
            raw: text,
        });
    }
    None
}

/// One network source, and the name it goes by in the config.
///
/// The list of these *is* the on/off switch: dropping a name skips that
/// provider, and the order of the list is the order they are consulted in. A
/// flag per provider would have needed four of them and still not expressed
/// the ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum Provider {
    Amll,
    LyricsPlus,
    LrcMux,
    LrcLib,
}

impl Provider {
    /// The documented order, and the default.
    pub const DEFAULT_ORDER: [Provider; 4] = [
        Provider::Amll,
        Provider::LyricsPlus,
        Provider::LrcMux,
        Provider::LrcLib,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Provider::Amll => "amll",
            Provider::LyricsPlus => "lyricsplus",
            Provider::LrcMux => "lrcmux",
            Provider::LrcLib => "lrclib",
        }
    }

    /// For the error message below, and for `--help`.
    pub fn names() -> String {
        Provider::DEFAULT_ORDER
            .map(Provider::name)
            .join(", ")
    }
}

impl std::str::FromStr for Provider {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let want = s.trim().to_ascii_lowercase();
        Provider::DEFAULT_ORDER
            .into_iter()
            .find(|p| p.name() == want)
            // Naming the valid ones matters more here than usual: this is a
            // typo in a config file, and an ignored one would look like the
            // provider was simply never reached.
            .ok_or_else(|| format!("unknown provider `{s}`; valid names are {}", Provider::names()))
    }
}

impl TryFrom<String> for Provider {
    type Error = String;

    fn try_from(s: String) -> std::result::Result<Self, String> {
        s.parse()
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// The network half of a lookup, behind a seam so the ordering logic can be
/// tested without one.
///
/// It is boxed and `Send` rather than an `async fn` in a trait because the
/// lookup is handed to `tokio::spawn`, which needs the future to be `Send`, and
/// because `tests/provider_order.rs` wants a `dyn` implementation to stand in
/// front of it.
pub trait Providers: Send + Sync {
    fn fetch<'a>(
        &'a self,
        provider: Provider,
        track: &'a Track,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Found>>> + Send + 'a>>;
}

/// The real providers, sharing one HTTP client.
pub struct Net {
    lrclib: LrcLib,
    lyricsplus_url: String,
    lrcmux_url: String,
}

impl Net {
    pub fn new(lyricsplus_url: String, lrcmux_url: String) -> Result<Self> {
        Ok(Self {
            lrclib: LrcLib::new()?,
            lyricsplus_url,
            lrcmux_url,
        })
    }
}

impl Providers for Net {
    fn fetch<'a>(
        &'a self,
        provider: Provider,
        track: &'a Track,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Found>>> + Send + 'a>> {
        Box::pin(async move {
            match provider {
                Provider::Amll => match amll::spotify_track_id(&track.id) {
                    Some(id) => amll::fetch(self.lrclib.http(), id).await,
                    // Not a Spotify track: there is no key to look up, and no
                    // request worth making.
                    None => Ok(None),
                },
                Provider::LyricsPlus => {
                    lyricsplus::fetch(self.lrclib.http(), &self.lyricsplus_url, track).await
                }
                Provider::LrcMux => {
                    lrcmux::fetch(self.lrclib.http(), &self.lrcmux_url, track).await
                }
                Provider::LrcLib => {
                    lrclib::fetch(
                        &self.lrclib,
                        &track.artist,
                        &track.title,
                        track.album.as_deref(),
                        track.length,
                    )
                    .await
                }
            }
        })
    }
}

/// Walk the chain until one provider answers *well*. No cache — this is what
/// `lyrics fetch` runs, and what [`lookup`] runs on a cache miss.
///
/// Word timings end the search; a line-level answer does not. The ordering
/// above is by what each source is usually good at, but "usually" is doing
/// real work there — LyricsPlus serves Apple's line-level document for a
/// minority of tracks, and returning that would step over word timings lrcmux
/// was holding all along. So a line-level answer is kept as a fallback and the
/// chain carries on; it is returned only once nothing better has turned up.
/// The first such answer wins rather than the last, which keeps the documented
/// ordering intact among answers of equal quality.
///
/// A provider that fails is logged and stepped over; losing LRCLIB's line-level
/// answer because a hobby-run service was down would be the worst possible
/// trade. `Err` is reserved for the case where *every* provider failed and none
/// of them gave a clean answer, so the UI can still tell a network outage from
/// a track that genuinely has no lyrics.
pub async fn first_hit(
    providers: &dyn Providers,
    order: &[Provider],
    track: &Track,
) -> Result<Option<Found>> {
    let mut answered = false;
    let mut last_error = None;
    let mut fallback: Option<Found> = None;

    for &provider in order {
        match providers.fetch(provider, track).await {
            Ok(Some(found)) => {
                if found.lyrics.has_word_timings() {
                    return Ok(Some(found));
                }
                answered = true;
                if fallback.is_none() {
                    debug(&format!("{provider} answered line-level; looking for better"));
                    fallback = Some(found);
                }
            }
            Ok(None) => answered = true,
            Err(e) => {
                debug(&format!("{provider} lookup failed: {e:#}"));
                last_error = Some(e);
            }
        }
    }

    // Nothing was word-timed. A line-level answer still beats no answer.
    if let Some(found) = fallback {
        return Ok(Some(found));
    }

    match last_error {
        // At least one working provider said "not here", which is real
        // information: treat it as a miss even though another one broke.
        Some(_) if answered => Ok(None),
        Some(e) => Err(e),
        None => Ok(None),
    }
}

/// Cache-aware lookup for a live track. Never returns an error for "no lyrics";
/// a network failure is reported so the UI can distinguish it from a real miss.
pub async fn lookup(
    providers: &dyn Providers,
    order: &[Provider],
    cache: &Cache,
    track: &Track,
) -> Result<Outcome> {
    let key = &track.id;
    if let Some(cached) = cache.get(key) {
        return Ok(match cached {
            Some(found) => Outcome::Found(Box::new(found)),
            None => Outcome::Missing,
        });
    }

    match first_hit(providers, order, track).await? {
        Some(found) => {
            let id = match found.source {
                Source::LrcLib { id } => Some(id),
                _ => None,
            };
            cache.put_hit(key, &track.label(), &found.raw, id, found.synced);
            Ok(Outcome::Found(Box::new(found)))
        }
        None => {
            cache.put_miss(key, &track.label());
            Ok(Outcome::Missing)
        }
    }
}

fn debug(msg: &str) {
    if std::env::var_os("LYRICS_DEBUG").is_some() {
        eprintln!("[lyrics] {msg}");
    }
}
