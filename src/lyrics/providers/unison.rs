//! Unison: community-submitted lyrics, as TTML, LRC or plain text — whatever
//! was submitted.

use std::time::Duration;

use serde::Deserialize;

use super::Http;
use crate::lyrics::error::Result;
use crate::lyrics::format::{lrc, ttml};
use crate::lyrics::model::{Lyrics, Request};

const URL: &str = "https://unison.betterlyrics.org/lyrics";

#[derive(Deserialize, Default)]
pub(crate) struct Payload {
    #[serde(default)]
    data: Data,
}

#[derive(Deserialize, Default)]
struct Data {
    #[serde(default)]
    lyrics: Option<String>,
    #[serde(default)]
    format: Option<String>,
}

pub async fn fetch(http: &Http, req: &Request) -> Result<Lyrics> {
    let query = req.query("song", "artist", "album", "duration");
    let payload: Payload = http.get_json(URL, &query, Duration::from_secs(8)).await?;
    decode(payload)
}

pub(crate) fn decode(payload: Payload) -> Result<Lyrics> {
    let text = payload.data.lyrics.unwrap_or_default();
    if payload.data.format.as_deref() == Some("ttml") {
        return ttml::parse(text.as_bytes());
    }
    lrc::parse(text.as_bytes())
}
