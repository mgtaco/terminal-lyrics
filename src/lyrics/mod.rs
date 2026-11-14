//! Order: `--lrc-dir`, a [`sidecar`] file, then the cache and the providers
//! raced by [`engine`].

pub mod cache;
pub mod display;
pub mod engine;
pub mod error;
pub mod format;
pub mod model;
pub mod providers;
pub mod sidecar;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

pub use self::engine::{Answer, Engine, Origin};
pub use self::error::Error;
use self::model::Request;
use crate::lrc::{self, Lyrics};
use crate::player::Track;

/// Where a set of lyrics came from — shown by `lyrics status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// `--lrc-dir`.
    LocalFile(PathBuf),
    /// A lyrics file next to the track.
    Sidecar(PathBuf),
    Provider {
        provider: Provider,
        cached: bool,
    },
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::LocalFile(p) | Source::Sidecar(p) => write!(f, "file {}", p.display()),
            Source::Provider {
                provider,
                cached: true,
            } => write!(f, "{provider} (cached)"),
            Source::Provider { provider, .. } => write!(f, "{provider}"),
        }
    }
}

impl Source {
    /// For the status strip, where a full path would crowd out everything else.
    pub fn short(&self) -> String {
        match self {
            Source::LocalFile(p) | Source::Sidecar(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string()),
            Source::Provider { provider, .. } => provider.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Found {
    pub lyrics: Lyrics,
    pub source: Source,
    /// False when only unsynced plain text was available.
    pub synced: bool,
    /// The lyrics as enhanced LRC, which is what `lyrics` was parsed from.
    pub raw: String,
    /// The provider's answer as it came, when there was one.
    pub answer: Option<Answer>,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Found(Box<Found>),
    /// The track is known to have no lyrics, or is instrumental, or has only
    /// plain text, which a display built on timing cannot show.
    Missing,
}

/// `Artist - Title.lrc` next to the user's own files, checked before anything.
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
            answer: None,
        });
    }
    None
}

/// The config's list of these is both the on/off switch and the priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub enum Provider {
    BetterLyrics,
    Unison,
    LyricsPlus,
    KuGou,
    LrcLib,
    YouTubeMusic,
}

impl Provider {
    /// The documented order, and the default.
    pub const DEFAULT_ORDER: [Provider; 6] = [
        Provider::BetterLyrics,
        Provider::Unison,
        Provider::LyricsPlus,
        Provider::LrcLib,
        Provider::YouTubeMusic,
        // Last: its timings are often off, so it is only worth having when
        // nothing else has the song word-timed.
        Provider::KuGou,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Provider::BetterLyrics => "betterlyrics",
            Provider::Unison => "unison",
            Provider::LyricsPlus => "lyricsplus",
            Provider::KuGou => "kugou",
            Provider::LrcLib => "lrclib",
            Provider::YouTubeMusic => "youtubemusic",
        }
    }

    /// Whether it usually has word timing — worth holding a line-synced
    /// winner for. LRCLIB has it too rarely; YouTube Music never does.
    pub fn word_sync(self) -> bool {
        !matches!(self, Provider::LrcLib | Provider::YouTubeMusic)
    }

    /// For the error message below, and for `--help`.
    pub fn names() -> String {
        Provider::DEFAULT_ORDER.map(Provider::name).join(", ")
    }
}

impl std::str::FromStr for Provider {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let want = s.trim().to_ascii_lowercase();
        if let Some(p) = Provider::DEFAULT_ORDER
            .into_iter()
            .find(|p| p.name() == want)
        {
            return Ok(p);
        }
        // Name the valid ones: an ignored typo here would look like the
        // provider was never reached.
        let why = match want.as_str() {
            "lrcmux" => " (removed in 0.3.0)",
            "amll" => " (removed after 0.5.0)",
            _ => "",
        };
        Err(format!(
            "unknown provider `{s}`{why}; valid names are {}",
            Provider::names()
        ))
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

/// Title trimmed, artists joined, a length only over a second, and the file URL
/// only when it applies.
pub fn request_for(track: &Track, network: bool) -> Request {
    Request {
        title: track.title.trim().to_string(),
        artist: track.artist.clone(),
        album: track.album.clone().unwrap_or_default(),
        duration: track.length.filter(|l| *l > 1.0).unwrap_or(0.0),
        file_url: track
            .url
            .clone()
            .filter(|u| u.starts_with("file://"))
            .unwrap_or_default(),
        cache_only: !network,
    }
}

/// What the lookup for this track is keyed on. Two tracks with the same key
/// get the same lyrics; a track whose key changes needs looking up again.
pub fn lookup_key(track: &Track) -> String {
    cache::track_key(&request_for(track, false))
}

/// An answer as something to show, or `Missing` when there is nothing timed.
pub fn outcome_from(answer: Answer) -> Outcome {
    if !answer.found {
        return Outcome::Missing;
    }
    let raw = display::to_lrc(&answer.lyrics);
    let lyrics = lrc::parse(&raw);
    if lyrics.is_empty() {
        return Outcome::Missing;
    }
    let source = match &answer.origin {
        Origin::Sidecar(path) => Source::Sidecar(path.clone()),
        Origin::Provider(provider) => Source::Provider {
            provider: *provider,
            cached: answer.cached,
        },
        Origin::Nowhere => return Outcome::Missing,
    };
    Outcome::Found(Box::new(Found {
        synced: !answer.lyrics.synced.is_empty(),
        lyrics,
        source,
        raw,
        answer: Some(answer),
    }))
}

/// The full lookup for a track. Never an error for "no lyrics"; a network
/// failure is reported so the UI can tell it from a real miss.
pub async fn lookup(
    engine: &Arc<Engine>,
    order: &[Provider],
    track: &Track,
    network: bool,
) -> Result<Outcome, Error> {
    lookup_with(engine, order, track, network, |_| {}).await
}

/// [`lookup`], with `early` handed each better stand-in found while a slower
/// provider could still beat it.
pub async fn lookup_with(
    engine: &Arc<Engine>,
    order: &[Provider],
    track: &Track,
    network: bool,
    mut early: impl FnMut(Outcome) + Send,
) -> Result<Outcome, Error> {
    let req = request_for(track, network);
    let answer = engine
        .lookup_with(&req, order, |a| early(outcome_from(a.clone())))
        .await?;
    Ok(outcome_from(answer))
}

/// What can be shown right now without waiting: a sidecar or a cached answer.
/// `None` means nothing is known yet.
pub fn cached(engine: &Engine, order: &[Provider], track: &Track) -> Option<Outcome> {
    let req = request_for(track, false);
    engine.cached(&req, order).map(outcome_from)
}
