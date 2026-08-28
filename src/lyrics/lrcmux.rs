//! lrcmux: one API in front of Musixmatch richsync, KuGou, NetEase, Genius and
//! YouTube Music.
//!
//! Where AMLL is deep and narrow — a couple of thousand tracks, always
//! word-timed — this is wide and uneven. It answers for most things, and
//! `meta.level` says outright whether the answer carries word timings, so
//! nothing has to be sniffed.
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
pub async fn fetch(http: &reqwest::Client, base: &str, track: &Track) -> Result<Option<Found>> {
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

    let resp = http
        .get(format!("{}/get", base.trim_end_matches('/')))
        .query(&params)
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
