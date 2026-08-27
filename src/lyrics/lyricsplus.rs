//! LyricsPlus: Apple Music's own lyrics, in Apple's own TTML.
//!
//! This is the best-looking source here. Apple times *syllables*, not words —
//! `be` then `lieve` — which is what drives the build-a-long-word-up-in-place
//! behaviour the renderer implements; the other providers can only light a
//! whole word at once.
//!
//! The service wraps the document in `{"ttml": "…"}` and nothing else, so the
//! whole provider is one request, one unwrap, and the TTML converter that AMLL
//! already goes through.
//!
//! One thing it does *not* do, despite documenting the parameter: resolve a
//! Spotify ID. `platformId=spotify:<id>` comes back `404` from the public
//! instance with an empty `songInfo`, on tracks the same instance answers for
//! by name — it has no Spotify credentials to resolve the ID with. So this
//! matches on artist and title like LRCLIB does, and leans on `dur` to throw
//! out a wrong match.

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{Found, MAX_DURATION_DELTA, Source, ttml};
use crate::lrc;
use crate::player::Track;

#[derive(Debug, Clone, Deserialize)]
struct Envelope {
    #[serde(default)]
    ttml: Option<String>,
}

/// Ask LyricsPlus for one track. `Ok(None)` covers a miss, a document that does
/// not parse, and a document timed against a different edit — all of which mean
/// "ask the next provider", not "the lookup failed".
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
        .get(format!("{}/v1/ttml/get", base.trim_end_matches('/')))
        .query(&params)
        .send()
        .await
        .context("LyricsPlus request failed")?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp = resp
        .error_for_status()
        .context("LyricsPlus returned an error")?;
    let envelope = resp
        .json::<Envelope>()
        .await
        .context("bad JSON from LyricsPlus")?;

    let Some(xml) = envelope.ttml.filter(|t| !t.trim().is_empty()) else {
        return Ok(None);
    };
    Ok(from_ttml(&xml, track.length))
}

/// The pure half of [`fetch`].
pub fn from_ttml(xml: &str, want_duration: Option<f64>) -> Option<Found> {
    // A different edit of the song is wrong from the first line to the last,
    // and Apple records the length it timed against — so this is checkable
    // rather than a guess.
    if let (Some(want), Some(have)) = (want_duration, ttml::document_duration(xml))
        && (want - have).abs() > MAX_DURATION_DELTA
    {
        return None;
    }

    // A malformed document falls through to the next provider rather than
    // failing the lookup, exactly as an unusable AMLL entry does.
    let a2 = ttml::to_enhanced_lrc(xml).ok()?;
    let lyrics = lrc::parse(&a2);
    if lyrics.is_empty() {
        return None;
    }

    Some(Found {
        lyrics,
        source: Source::LyricsPlus,
        synced: true,
        raw: a2,
    })
}
