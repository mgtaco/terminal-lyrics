//! lrcmux: one API in front of Musixmatch richsync, KuGou, LRCLIB, Genius and
//! YouTube Music.
//!
//! Where AMLL is deep and narrow — a couple of thousand tracks, always
//! word-timed — this is wide and uneven. It answers for most things, and
//! `meta.level` says outright whether the answer carries word timings, so
//! nothing has to be sniffed.
//!
//! The upstreams are not equally good, and lrcmux picks between them itself:
//! asking for `musixmatch,kugou` and for `kugou,musixmatch` returns the same
//! answer, so the order of [`Sources`] is not a preference the server honours.
//! What it does honour is the set — which is why the knob is a filter rather
//! than a ranking, and why the default excludes one upstream outright. See
//! [`Sources::DEFAULT`].
//!
//! The upstreams do not agree on how a word is written, and the difference is
//! invisible until it is on screen:
//!
//! ```text
//! musixmatch  {"text": "When "}  {"text": "you "}   — the space rides along
//! kugou       {"text": "Du"} {"text": ","} {"text": " "}  — the space is its own span
//! ```
//!
//! Both reconstruct the line by plain concatenation, which is what the
//! conversion below relies on: it copies each word's own whitespace through
//! verbatim and puts the timing tags around what is left. Guessing instead —
//! joining words with a space — would double the spaces in one dialect and
//! wrap punctuation in tags of its own in the other. Getting it backwards and
//! emitting no space at all is worse still: `Line::continues_word` reads
//! exactly that whitespace to tell a word from a syllable, so a spaceless line
//! displays as one enormous unbroken word.

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{Found, MAX_DURATION_DELTA, Source, ttml};
use crate::lrc;
use crate::player::Track;

/// Which of lrcmux's upstreams to let answer.
///
/// The server takes a `sources` parameter that is either an allow-list
/// (`musixmatch,ytmusic`) or a deny-list (`!kugou`), never a mix of the two —
/// so this refuses a mix rather than sending something the server has to guess
/// at. An empty list sends no parameter at all and lets lrcmux choose freely.
///
/// The names themselves are *not* checked against a fixed set: the list of
/// upstreams belongs to the server and grew and shrank twice while this file
/// was being written. `lyrics status` names the upstream that actually
/// answered, which is the honest way to find out whether a name did anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sources(Vec<String>);

impl Sources {
    /// KuGou excluded, everything else allowed.
    ///
    /// Measured over 111 tracks, KuGou's lyrics overlapped known-good words
    /// with a median Jaccard of 0.86 against Musixmatch's 0.97, and its lower
    /// quartile ran down to 0.20 — whole songs of confidently wrong words,
    /// which is what an automatic transcription of English by a Chinese service
    /// looks like. Wrong words cannot be nudged back into place the way a wrong
    /// offset can, and lrcmux prefers KuGou when both answer, so excluding it
    /// is the only thing that helps. Anyone who wants it back names it in the
    /// config.
    pub const DEFAULT: &'static str = "!kugou";

    /// Everything lrcmux is willing to serve.
    pub fn any() -> Self {
        Sources(Vec::new())
    }

    /// The `sources` query parameter, or `None` to leave it off entirely.
    pub fn param(&self) -> Option<String> {
        (!self.0.is_empty()).then(|| self.0.join(","))
    }

    pub fn names(&self) -> &[String] {
        &self.0
    }
}

impl std::str::FromStr for Sources {
    type Err = String;

    /// Comma-separated, each name optionally `!`-prefixed to exclude it.
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        Sources::try_from(
            s.split(',')
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect::<Vec<_>>(),
        )
    }
}

impl TryFrom<Vec<String>> for Sources {
    type Error = String;

    fn try_from(names: Vec<String>) -> std::result::Result<Self, String> {
        let mut cleaned = Vec::new();
        for name in names {
            let name = name.trim().to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }
            let bare = name.strip_prefix('!').unwrap_or(&name);
            if bare.is_empty() {
                return Err("`!` needs an upstream name after it".to_string());
            }
            // A name with a comma in it would split into two on the wire and
            // mean something the user never wrote.
            if bare.contains([',', ' ']) {
                return Err(format!(
                    "`{name}` is not one upstream name; list them separately"
                ));
            }
            cleaned.push(name);
        }
        // Mixing the two forms is not a stricter filter, it is a contradiction:
        // an allow-list already excludes everything it does not name.
        let excluding = cleaned.iter().filter(|n| n.starts_with('!')).count();
        if excluding != 0 && excluding != cleaned.len() {
            return Err(
                "lrcmux sources are either all excluded (`!kugou`) or all allowed \
                 (`musixmatch`), not a mix of both"
                    .to_string(),
            );
        }
        Ok(Sources(cleaned))
    }
}

impl<'de> Deserialize<'de> for Sources {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Vec::<String>::deserialize(d)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for Sources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.join(","))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub track: Option<TrackInfo>,
    pub meta: Meta,
    #[serde(default)]
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackInfo {
    /// Whole seconds, when the upstream knows it.
    #[serde(default)]
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    pub level: Level,
    #[serde(default)]
    pub source: Option<SourceInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceInfo {
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Word,
    Line,
    None,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Line {
    #[serde(default)]
    pub text: String,
    /// Milliseconds, like every other time in this API.
    #[serde(default)]
    pub start: i64,
    #[serde(default)]
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Word {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub start: i64,
    #[serde(default)]
    pub end: i64,
}

fn seconds(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

/// Split a word into (leading whitespace, the word itself, trailing whitespace).
fn split_padding(text: &str) -> (&str, &str, &str) {
    let core = text.trim();
    if core.is_empty() {
        return (text, "", "");
    }
    let start = text.find(core).unwrap_or(0);
    (&text[..start], core, &text[start + core.len()..])
}

/// The A2 body for one word-timed line, or `None` when the words do not add up
/// to the line's own text.
///
/// That check is the safety net for an upstream this code has not seen. If the
/// words cannot be trusted to reconstruct the line, the caller keeps the line
/// at line granularity rather than showing text that is missing pieces of
/// itself.
fn word_body(line: &Line) -> Option<String> {
    let mut body = String::new();
    let mut any = false;
    for w in &line.words {
        let (before, core, after) = split_padding(&w.text);
        body.push_str(before);
        if !core.is_empty() {
            body.push_str(&format!("<{}>", ttml::format_time(seconds(w.start))));
            body.push_str(core);
            body.push_str(&format!("<{}>", ttml::format_time(seconds(w.end))));
            any = true;
        }
        body.push_str(after);
    }
    if !any {
        return None;
    }

    // Compare against the line's own text with whitespace runs collapsed: the
    // upstreams differ on padding, but never on the words themselves.
    let rebuilt: String = line.words.iter().map(|w| w.text.as_str()).collect();
    (squash(&rebuilt) == squash(&line.text)).then_some(body)
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Convert a response into enhanced LRC, or `None` if there is nothing timed
/// to show.
pub fn to_enhanced_lrc(resp: &Response) -> Option<String> {
    if resp.meta.level == Level::None {
        return None;
    }

    let mut out = String::new();
    for line in &resp.lines {
        let body = match resp.meta.level {
            Level::Word => word_body(line).unwrap_or_else(|| line.text.trim().to_string()),
            // Line level has nothing to split on; emit the plain timed line.
            _ => line.text.trim().to_string(),
        };
        out.push_str(&format!(
            "[{}]{}\n",
            ttml::format_time(seconds(line.start)),
            body.trim()
        ));
    }
    (!out.trim().is_empty()).then_some(out)
}

/// Ask lrcmux for one track. `Ok(None)` is a real miss, or an answer with no
/// timings worth showing; the chain then moves on to the next provider.
/// The URL one lookup is made against.
///
/// Split out from [`fetch`] because one detail of it is load-bearing and
/// invisible: see the `sources` handling below.
pub fn request_url(base: &str, sources: &Sources, track: &Track) -> Result<reqwest::Url> {
    let mut params: Vec<(&str, String)> = vec![
        ("artist", track.artist.clone()),
        ("title", track.title.clone()),
    ];
    if let Some(a) = track.album.as_deref().filter(|a| !a.trim().is_empty()) {
        params.push(("album", a.to_string()));
    }
    if let Some(d) = track.length {
        params.push(("duration", format!("{}", d.round() as i64)));
    }

    let mut url = reqwest::Url::parse(&format!("{}/get", base.trim_end_matches('/')))
        .context("lrcmux base URL is not a URL")?;
    url.query_pairs_mut().extend_pairs(&params);

    // `sources` is appended by hand rather than through `query_pairs_mut`,
    // which form-encodes `!` to `%21`. lrcmux does not percent-decode this one
    // parameter: it reads `%21kugou` as an upstream literally called that,
    // matches none, restricts the fanout to nothing and answers 404 — so the
    // filter would silently turn lrcmux off altogether instead of narrowing it,
    // which looks from the outside exactly like the service being down.
    // `Url::set_query` leaves `!` and `,` alone, and `Sources` has already
    // refused anything else that would need encoding.
    if let Some(s) = sources.param() {
        let rest = url.query().unwrap_or_default().to_string();
        let sep = if rest.is_empty() { "" } else { "&" };
        url.set_query(Some(&format!("{rest}{sep}sources={s}")));
    }
    Ok(url)
}

pub async fn fetch(
    http: &reqwest::Client,
    base: &str,
    sources: &Sources,
    track: &Track,
) -> Result<Option<Found>> {
    let resp = http
        .get(request_url(base, sources, track)?)
        .send()
        .await
        .context("lrcmux request failed")?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp = resp
        .error_for_status()
        .context("lrcmux returned an error")?;
    let body = resp
        .json::<Response>()
        .await
        .context("bad JSON from lrcmux")?;

    Ok(from_response(&body, track.length))
}

/// The pure half of [`fetch`], so the conversion is testable without a network.
pub fn from_response(resp: &Response, want_duration: Option<f64>) -> Option<Found> {
    // A different edit of the song is wrong from the first line to the last —
    // the same rule LRCLIB matching applies, for the same reason.
    if let (Some(want), Some(have)) = (want_duration, resp.track.as_ref().and_then(|t| t.duration))
        && (want - have).abs() > MAX_DURATION_DELTA
    {
        return None;
    }

    let a2 = to_enhanced_lrc(resp)?;
    let lyrics = lrc::parse(&a2);
    if lyrics.is_empty() {
        return None;
    }

    let provider = resp
        .meta
        .source
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    Some(Found {
        lyrics,
        source: Source::LrcMux { provider },
        synced: true,
        raw: a2,
    })
}
