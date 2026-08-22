//! LRCLIB client.
//!
//! v1 took `results[0]` from a search and hoped. That is measurably wrong: a
//! search for "Kanye West – Flashing Lights" returns twenty rows whose
//! durations run 198s to 323s, and the first row is an 85-second-longer edit
//! than the album track. Here the track length the player already told us is
//! used to score candidates, and anything more than a few seconds off is
//! rejected outright.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::cache::Cache;
use super::normalize;
use super::{Found, Outcome, Source};
use crate::lrc;
use crate::player::Track;

const API: &str = "https://lrclib.net/api";
const USER_AGENT: &str = concat!(
    "terminal-lyrics/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/tacoproz1/terminal-lyrics)"
);
/// Beyond this the "match" is a different edit of the song and the timings are
/// wrong throughout — worse than showing nothing.
const MAX_DURATION_DELTA: f64 = 5.0;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: i64,
    #[serde(default)]
    pub track_name: String,
    #[serde(default)]
    pub artist_name: String,
    #[serde(default)]
    pub album_name: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub instrumental: bool,
    #[serde(default)]
    pub plain_lyrics: Option<String>,
    #[serde(default)]
    pub synced_lyrics: Option<String>,
}

impl Record {
    fn best_text(&self) -> Option<(&str, bool)> {
        if let Some(s) = self.synced_lyrics.as_deref().filter(|s| !s.trim().is_empty()) {
            return Some((s, true));
        }
        self.plain_lyrics
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| (s, false))
    }

    fn into_found(self) -> Option<Found> {
        let (text, synced) = self.best_text()?;
        let lyrics = lrc::parse(text);
        if lyrics.is_empty() {
            return None;
        }
        Some(Found {
            lyrics,
            source: Source::LrcLib { id: self.id },
            synced,
            raw: text.to_string(),
        })
    }
}

pub struct LrcLib {
    http: reqwest::Client,
}

impl LrcLib {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()
            .context("failed to build the HTTP client")?;
        Ok(Self { http })
    }

    /// Exact lookup. 404 is a normal answer, not an error.
    async fn get(
        &self,
        artist: &str,
        title: &str,
        album: Option<&str>,
        duration: Option<f64>,
    ) -> Result<Option<Record>> {
        let mut params: Vec<(&str, String)> = vec![
            ("artist_name", artist.to_string()),
            ("track_name", title.to_string()),
        ];
        if let Some(a) = album.filter(|a| !a.trim().is_empty()) {
            params.push(("album_name", a.to_string()));
        }
        if let Some(d) = duration {
            params.push(("duration", format!("{}", d.round() as i64)));
        }

        let resp = self
            .http
            .get(format!("{API}/get"))
            .query(&params)
            .send()
            .await
            .context("LRCLIB request failed")?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let resp = resp.error_for_status().context("LRCLIB returned an error")?;
        Ok(Some(resp.json::<Record>().await.context("bad JSON from LRCLIB")?))
    }

    async fn search(&self, artist: &str, title: &str) -> Result<Vec<Record>> {
        let resp = self
            .http
            .get(format!("{API}/search"))
            .query(&[("artist_name", artist), ("track_name", title)])
            .send()
            .await
            .context("LRCLIB search failed")?
            .error_for_status()
            .context("LRCLIB search returned an error")?;
        Ok(resp.json::<Vec<Record>>().await.unwrap_or_default())
    }
}

/// Lower is better. `None` means "reject outright".
fn score(rec: &Record, want_duration: Option<f64>, want_title: &str, want_artist: &str) -> Option<f64> {
    rec.best_text()?;
    let mut score = 0.0;

    if let (Some(want), Some(have)) = (want_duration, rec.duration) {
        let delta = (want - have).abs();
        if delta > MAX_DURATION_DELTA {
            return None; // a different edit of the song
        }
        score += delta;
    } else if want_duration.is_some() && rec.duration.is_none() {
        // Unknown length is a mild risk when we could have checked.
        score += 2.0;
    }

    // Synced lyrics are the entire point of this program.
    if rec.synced_lyrics.is_none() {
        score += 20.0;
    }
    // An instrumental record has nothing to show, but beats nothing at all.
    if rec.instrumental {
        score += 50.0;
    }
    if !rec.track_name.eq_ignore_ascii_case(want_title) {
        score += 1.0;
    }
    if !rec.artist_name.eq_ignore_ascii_case(want_artist) {
        score += 1.0;
    }
    Some(score)
}

/// Best candidate from a search result set, or `None` if none is acceptable.
pub fn pick_best(
    records: Vec<Record>,
    want_duration: Option<f64>,
    want_title: &str,
    want_artist: &str,
) -> Option<Record> {
    records
        .into_iter()
        .filter_map(|r| score(&r, want_duration, want_title, want_artist).map(|s| (s, r)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, r)| r)
}

/// The full lookup ladder for one track.
pub async fn fetch(
    client: &LrcLib,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration: Option<f64>,
) -> Result<Option<Found>> {
    // 1. Exact, with everything we know.
    if let Some(rec) = client.get(artist, title, album, duration).await?
        && let Some(found) = rec.into_found()
    {
        return Ok(Some(found));
    }

    // 2. Exact, without the album — album names differ across releases.
    if album.is_some()
        && let Some(rec) = client.get(artist, title, None, duration).await?
        && let Some(found) = rec.into_found()
    {
        return Ok(Some(found));
    }

    // 3. Search, scored by how close the length is to the track being played.
    let results = client.search(artist, title).await.unwrap_or_default();
    if let Some(rec) = pick_best(results, duration, title, artist)
        && let Some(found) = rec.into_found()
    {
        return Ok(Some(found));
    }

    // 4. Retry with the decorations stripped: `- Remastered 2011`, `(feat. …)`.
    let relaxed_title = normalize::relax_title(title);
    let relaxed_artist = normalize::primary_artist(artist);
    if relaxed_title.is_some() || relaxed_artist.is_some() {
        let t = relaxed_title.as_deref().unwrap_or(title);
        let a = relaxed_artist.as_deref().unwrap_or(artist);
        if let Some(rec) = client.get(a, t, None, duration).await?
            && let Some(found) = rec.into_found()
        {
            return Ok(Some(found));
        }
        let results = client.search(a, t).await.unwrap_or_default();
        if let Some(rec) = pick_best(results, duration, t, a)
            && let Some(found) = rec.into_found()
        {
            return Ok(Some(found));
        }
    }

    Ok(None)
}

/// Cache-aware lookup for a live track. Never returns an error for "no lyrics";
/// a network failure is reported so the UI can distinguish it from a real miss.
pub async fn lookup(
    client: &LrcLib,
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

    let found = fetch(
        client,
        &track.artist,
        &track.title,
        track.album.as_deref(),
        track.length,
    )
    .await?;

    match found {
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
