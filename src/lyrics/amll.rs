//! The AMLL TTML database: community-maintained word-by-word lyrics, keyed by
//! Spotify track ID and published under CC0.
//!
//! This is the only source here that carries real per-word timings — LRCLIB
//! stores line-level LRC only (0 of 923 synced entries sampled had word tags).
//! Coverage is the trade: a couple of thousand tracks against LRCLIB's
//! millions, so this is tried first and LRCLIB catches everything it misses.
//!
//! The lookup needs no search and no scoring: the MPRIS `xesam:url` Spotify
//! already gives us *is* the key.

use anyhow::{Context, Result};

use super::{Found, Source, ttml};
use crate::lrc;

const BASE: &str = "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/main/spotify-lyrics";

/// Pull the track ID out of whatever the player called the track.
///
/// Spotify reports `xesam:url` as `https://open.spotify.com/track/<id>`, which
/// is what [`crate::player::Track::id`] holds; a bare ID or a `spotify:track:`
/// URI is accepted too.
pub fn spotify_track_id(key: &str) -> Option<&str> {
    let key = key.trim();
    let candidate = if let Some(rest) = key.split_once("open.spotify.com/track/") {
        rest.1
    } else if let Some(rest) = key.strip_prefix("spotify:track:") {
        rest
    } else {
        key
    };
    // Trim a query string or trailing path segment.
    let candidate = candidate.split(['?', '#', '/']).next().unwrap_or_default();

    // Spotify IDs are 22 base62 characters. Checking this keeps a filename or a
    // free-text key from becoming a request for a URL that cannot exist.
    (candidate.len() == 22 && candidate.chars().all(|c| c.is_ascii_alphanumeric()))
        .then_some(candidate)
}

/// Fetch word-timed lyrics for a Spotify track ID. `Ok(None)` means the
/// database simply does not have this track, which is the common case.
pub async fn fetch(http: &reqwest::Client, spotify_id: &str) -> Result<Option<Found>> {
    let url = format!("{BASE}/{spotify_id}.ttml");
    let resp = http.get(&url).send().await.context("AMLL request failed")?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp = resp.error_for_status().context("AMLL returned an error")?;
    let xml = resp
        .text()
        .await
        .context("could not read the AMLL response")?;

    // A malformed entry should fall through to LRCLIB, not fail the lookup.
    let Ok(a2) = ttml::to_enhanced_lrc(&xml) else {
        return Ok(None);
    };
    let lyrics = lrc::parse(&a2);
    if lyrics.is_empty() {
        return Ok(None);
    }

    Ok(Some(Found {
        lyrics,
        source: Source::Amll,
        synced: true,
        raw: a2,
    }))
}
