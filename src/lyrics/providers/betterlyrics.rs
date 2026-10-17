//! Better Lyrics: Apple Music's TTML, word-timed with voices and backing
//! vocals, from the server behind the Better Lyrics browser extension.

use std::time::Duration;

use serde::Deserialize;

use super::Http;
use crate::lyrics::error::{Error, Result};
use crate::lyrics::format::ttml;
use crate::lyrics::model::{Lyrics, Request};

const URL: &str = "https://lyrics-api.boidu.dev/getLyrics";

#[derive(Deserialize)]
struct Payload {
    #[serde(default)]
    ttml: Option<String>,
}

pub async fn fetch(http: &Http, req: &Request) -> Result<Lyrics> {
    let query = req.query("s", "a", "al", "d");
    let payload: Payload = http.get_json(URL, &query, Duration::from_secs(4)).await?;
    match payload.ttml.filter(|t| !t.is_empty()) {
        Some(doc) => ttml::parse(doc.as_bytes()),
        None => Err(Error::NotFound),
    }
}
