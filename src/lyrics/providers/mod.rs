//! The six built-in providers, and the HTTP they share.
//!
//! Each provider is a fetch function that turns a [`Request`] into
//! [`Lyrics`]: empty lyrics for "nothing here", an error for "could not ask".
//! What happens around the fetch — the cache, the rate limit, the race between
//! providers — is the same for every one of them and lives in
//! [`super::engine`].

pub mod betterlyrics;
pub mod kugou;
pub mod lrclib;
pub mod lyricsplus;
pub mod unison;
pub mod youtubemusic;

use std::time::Duration;

use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use super::error::{Error, Result};
use super::format::MAX_SOURCE_BYTES;

pub const USER_AGENT: &str = concat!(
    "terminal-lyrics/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/mgtaco/terminal-lyrics)"
);

/// One HTTP client for every provider.
pub struct Http {
    client: reqwest::Client,
}

impl Http {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(3))
            .pool_idle_timeout(Duration::from_secs(90))
            // A provider has no business sending us anywhere else.
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let origin = &attempt.previous()[0];
                let url = attempt.url();
                if attempt.previous().len() >= 5
                    || url.scheme() != origin.scheme()
                    || url.host_str() != origin.host_str()
                    || url.port_or_known_default() != origin.port_or_known_default()
                {
                    attempt.error("refusing lyrics redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        Ok(Self { client })
    }

    /// GET `endpoint?query`, bounded in time and size.
    pub(crate) async fn get(
        &self,
        endpoint: &str,
        query: &[(String, String)],
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        let url = format!("{endpoint}?{}", encode_query(query));
        self.send(self.client.get(url).timeout(timeout)).await
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        query: &[(String, String)],
        timeout: Duration,
    ) -> Result<T> {
        decode_json(&self.get(endpoint, query, timeout).await?)
    }

    pub(crate) async fn send(&self, request: reqwest::RequestBuilder) -> Result<Vec<u8>> {
        let mut response = request
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(classify_status(status));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_SOURCE_BYTES {
                return Err(Error::TooLarge);
            }
        }
        Ok(body)
    }

    pub(crate) fn client(&self) -> &reqwest::Client {
        &self.client
    }
}

/// Only a transient status is worth reporting as an error. BetterLyrics
/// answers 401 for a track its server has not cached yet.
pub(crate) fn classify_status(status: StatusCode) -> Error {
    match status {
        StatusCode::UNAUTHORIZED => Error::Uncached,
        StatusCode::NOT_FOUND | StatusCode::FORBIDDEN | StatusCode::GONE => Error::NotFound,
        other => Error::Status(other.as_u16()),
    }
}

pub(crate) fn decode_json<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    serde_json::from_slice(body)
        .map_err(|e| Error::invalid(format!("invalid lyrics response: {e}")))
}

/// Sorted by key, everything but `A-Za-z0-9-_.~` percent-encoded, and a space
/// as `%20` rather than `+`: KuGou's search finds nothing for `a - b` when the
/// spaces arrive as `+`. Byte for byte what DMS sends.
pub(crate) fn encode_query(query: &[(String, String)]) -> String {
    let mut pairs: Vec<&(String, String)> = query.iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", escape(k), escape(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_is_sorted_and_spaces_are_percent_twenty() {
        let query = vec![
            ("s".to_string(), "Get Lucky".to_string()),
            ("a".to_string(), "Daft Punk & co+".to_string()),
        ];
        assert_eq!(
            encode_query(&query),
            "a=Daft%20Punk%20%26%20co%2B&s=Get%20Lucky"
        );
    }

    #[test]
    fn status_classes() {
        assert!(matches!(
            classify_status(StatusCode::UNAUTHORIZED),
            Error::Uncached
        ));
        assert!(classify_status(StatusCode::NOT_FOUND).is_not_found());
        assert!(matches!(
            classify_status(StatusCode::BAD_GATEWAY),
            Error::Status(502)
        ));
    }
}
