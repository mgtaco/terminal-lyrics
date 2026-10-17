//! LRCLIB: line-synced LRC, and now and then a word-timed Lyricsfile.
//!
//! `/api/get` prefers sync only among records with the track's exact
//! signature, and the same song is published under many. A plain or missing
//! answer gets a title search instead, and the best record for the same track
//! and artist within a few seconds of the length wins.

use std::time::Duration;

use serde::Deserialize;

use super::Http;
use crate::lyrics::error::{Error, Result};
use crate::lyrics::format::{MAX_LINES, lrc, lyricsfile};
use crate::lyrics::model::{Lyrics, Request};

const GET_URL: &str = "https://lrclib.net/api/get";
const SEARCH_URL: &str = "https://lrclib.net/api/search";
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Record {
    #[serde(default)]
    pub track_name: Option<String>,
    #[serde(default)]
    pub artist_name: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub instrumental: Option<bool>,
    #[serde(default)]
    pub has_word_sync: Option<bool>,
    #[serde(default)]
    pub plain_lyrics: Option<String>,
    #[serde(default)]
    pub synced_lyrics: Option<String>,
    #[serde(default)]
    pub lyricsfile: Option<String>,
}

pub async fn fetch(http: &Http, req: &Request) -> Result<Lyrics> {
    let query = req.query("track_name", "artist_name", "album_name", "duration");
    let exact: Option<Record> = match http.get_json(GET_URL, &query, TIMEOUT).await {
        Ok(record) => record,
        Err(e) if e.is_not_found() => None,
        Err(e) => return Err(e),
    };
    if let Some(record) = &exact
        && (record.is_instrumental() || record.tier() > 0)
    {
        return decode(record);
    }
    // A failed search is an error even with a plain record in hand: a cached
    // plain answer would never be asked again.
    let search = vec![("track_name".to_string(), req.title.clone())];
    let records: Vec<Record> = http
        .get_json::<Option<Vec<Record>>>(SEARCH_URL, &search, TIMEOUT)
        .await?
        .unwrap_or_default();
    match (best(&records, req), &exact) {
        (Some(best), None) => decode(best),
        (Some(best), Some(_)) if best.tier() > 0 => decode(best),
        (_, Some(exact)) => decode(exact),
        (None, None) => Err(Error::NotFound),
    }
}

/// Search pads its results with other songs, other artists and other cuts of
/// the same recording.
pub(crate) fn best<'a>(records: &'a [Record], req: &Request) -> Option<&'a Record> {
    let mut best: Option<&Record> = None;
    for record in records {
        let artists = record.artists();
        if !req.matches_duration(record.seconds())
            || !req.matches_track(record.track_name.as_deref().unwrap_or_default(), &artists)
        {
            continue;
        }
        if best.is_none_or(|b| record.outranks(b, req)) {
            best = Some(record);
        }
    }
    best
}

impl Record {
    fn is_instrumental(&self) -> bool {
        self.instrumental.unwrap_or(false)
    }

    /// Within a sync tier the closest duration wins: a record timed for
    /// another cut drifts.
    fn outranks(&self, other: &Record, req: &Request) -> bool {
        if self.tier() != other.tier() {
            return self.tier() > other.tier();
        }
        req.duration_gap(self.seconds()) < req.duration_gap(other.seconds())
    }

    fn tier(&self) -> u8 {
        if self.has_word_sync.unwrap_or(false) {
            2
        } else if self.synced_lyrics.as_deref().is_some_and(|s| !s.is_empty()) {
            1
        } else {
            0
        }
    }

    fn seconds(&self) -> i64 {
        self.duration.unwrap_or(0.0).round() as i64
    }

    /// YouTube's auto-generated channels are `<artist> - Topic`, and uploads
    /// scraped from them keep it.
    fn artists(&self) -> Vec<&str> {
        let name = self.artist_name.as_deref().unwrap_or_default();
        let name = name.strip_suffix(" - Topic").unwrap_or(name);
        name.split([',', ';', '/', '&', '\0'])
            .filter(|s| !s.is_empty())
            .collect()
    }
}

pub(crate) fn decode(record: &Record) -> Result<Lyrics> {
    if record.is_instrumental() {
        return Ok(Lyrics {
            instrumental: true,
            ..Default::default()
        });
    }
    let mut lyricsfile_err = None;
    if let Some(doc) = record.lyricsfile.as_deref().filter(|d| !d.is_empty()) {
        match lyricsfile::parse(doc.as_bytes()) {
            Ok(parsed) if !parsed.is_empty() => return Ok(parsed),
            Ok(_) => {}
            Err(e) => lyricsfile_err = Some(e),
        }
    }
    let mut parsed = lrc::parse(
        record
            .synced_lyrics
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    )?;
    if parsed.plain.is_empty() {
        let plain = record.plain_lyrics.clone().unwrap_or_default();
        if plain.matches('\n').count() + 1 > MAX_LINES {
            return Err(Error::TooLarge);
        }
        parsed.plain = plain;
    }
    if parsed.is_empty()
        && let Some(e) = lyricsfile_err
    {
        return Err(e);
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, artist: &str, duration: f64, synced: &str) -> Record {
        Record {
            track_name: Some(name.into()),
            artist_name: Some(artist.into()),
            duration: Some(duration),
            synced_lyrics: Some(synced.into()),
            ..Default::default()
        }
    }

    #[test]
    fn search_prefers_sync_then_the_closest_length() {
        let req = Request {
            title: "Get Lucky".into(),
            artist: "Daft Punk".into(),
            duration: 248.0,
            ..Default::default()
        };
        let records = vec![
            record("Get Lucky", "Daft Punk", 248.0, ""),
            record("Get Lucky", "Daft Punk", 254.0, "[00:01.00]a"),
            record("Get Lucky", "Daft Punk - Topic", 249.0, "[00:01.00]b"),
            record("Get Lucky", "Someone Else", 248.0, "[00:01.00]c"),
            record("Get Lucky", "Daft Punk", 369.0, "[00:01.00]d"),
        ];
        let best = best(&records, &req).unwrap();
        assert_eq!(best.synced_lyrics.as_deref(), Some("[00:01.00]b"));
    }

    #[test]
    fn plain_record_keeps_its_text() {
        let rec = Record {
            plain_lyrics: Some("one\ntwo".into()),
            ..Default::default()
        };
        let parsed = decode(&rec).unwrap();
        assert!(parsed.synced.is_empty());
        assert_eq!(parsed.plain, "one\ntwo");
    }
}
