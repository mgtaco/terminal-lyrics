//! On-disk cache, positive and negative.
//!
//! The negative half matters as much as the positive one: without it, a track
//! LRCLIB has never heard of is re-queried on every track change, every restart,
//! forever. Misses expire after a day so newly uploaded lyrics are picked up.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{Found, Source};
use crate::lrc;

const MISS_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Bumped to 2: entries written before the TTML time-format fix are missing
/// every lyric line under a minute, and a stale hit would keep serving them.
///
/// Bumped to 3 for the LyricsPlus and lrcmux providers. Every track played
/// before them is cached with whatever LRCLIB had, which is line-level; `get`
/// returns that before a provider is ever consulted, so without this the two
/// new word-timed sources would be invisible across the user's entire existing
/// library — the feature would look like it had not shipped.
const CACHE_VERSION: u32 = 3;

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    version: u32,
    stored_at: u64,
    /// Present on a hit, absent on a miss.
    lrc: Option<String>,
    lrclib_id: Option<i64>,
    synced: bool,
    /// What the entry was for, kept for debugging by hand.
    label: String,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// FNV-1a over the track identity. Not cryptographic — it just has to turn an
/// arbitrary URL into a filename that survives every filesystem.
fn key_hash(key: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub struct Cache {
    dir: Option<PathBuf>,
}

impl Cache {
    /// A cache that cannot find its directory degrades to a no-op rather than
    /// failing the lookup.
    pub fn new(dir: Option<PathBuf>) -> Self {
        if let Some(d) = &dir
            && let Err(e) = std::fs::create_dir_all(d)
        {
            debug(&format!("cache disabled ({}): {e}", d.display()));
            return Self { dir: None };
        }
        Self { dir }
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("{}.json", key_hash(key))))
    }

    /// `Some(Some(found))` = hit, `Some(None)` = a remembered miss,
    /// `None` = nothing cached, go and look.
    pub fn get(&self, key: &str) -> Option<Option<Found>> {
        let path = self.path(key)?;
        let text = std::fs::read_to_string(&path).ok()?;
        let entry: Entry = serde_json::from_str(&text).ok()?;
        if entry.version != CACHE_VERSION {
            return None;
        }

        match entry.lrc {
            Some(text) => {
                let lyrics = lrc::parse(&text);
                if lyrics.is_empty() {
                    return None;
                }
                Some(Some(Found {
                    lyrics,
                    source: Source::Cache,
                    synced: entry.synced,
                    raw: text,
                }))
            }
            None => {
                let age = now_secs().saturating_sub(entry.stored_at);
                if Duration::from_secs(age) > MISS_TTL {
                    None // expired; ask again
                } else {
                    Some(None)
                }
            }
        }
    }

    pub fn put_hit(&self, key: &str, label: &str, lrc_text: &str, lrclib_id: Option<i64>, synced: bool) {
        self.write(
            key,
            Entry {
                version: CACHE_VERSION,
                stored_at: now_secs(),
                lrc: Some(lrc_text.to_string()),
                lrclib_id,
                synced,
                label: label.to_string(),
            },
        );
    }

    pub fn put_miss(&self, key: &str, label: &str) {
        self.write(
            key,
            Entry {
                version: CACHE_VERSION,
                stored_at: now_secs(),
                lrc: None,
                lrclib_id: None,
                synced: false,
                label: label.to_string(),
            },
        );
    }

    /// Forget one entry, so `r` in the TUI can force a fresh lookup.
    pub fn forget(&self, key: &str) {
        if let Some(p) = self.path(key) {
            let _ = std::fs::remove_file(p);
        }
    }

    fn write(&self, key: &str, entry: Entry) {
        let Some(path) = self.path(key) else { return };
        let Ok(json) = serde_json::to_string(&entry) else {
            return;
        };
        // Write-then-rename: a killed process must not leave a half-written
        // file that then parses as garbage forever.
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

fn debug(msg: &str) {
    if std::env::var_os("LYRICS_DEBUG").is_some() {
        eprintln!("[lyrics] {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::key_hash;

    #[test]
    fn key_hash_matches_the_fnv_1a_reference_vectors() {
        // Pinned so a mistyped constant cannot silently change every cache
        // filename — which is exactly what an underscore in the wrong place
        // did during development.
        assert_eq!(key_hash(""), "cbf29ce484222325");
        assert_eq!(key_hash("a"), "af63dc4c8601ec8c");
        assert_eq!(key_hash("foobar"), "85944171f73967e8");
    }

    #[test]
    fn distinct_keys_get_distinct_files() {
        let a = key_hash("https://open.spotify.com/track/aaaaaaaaaaaaaaaaaaaaaa");
        let b = key_hash("https://open.spotify.com/track/aaaaaaaaaaaaaaaaaaaaab");
        assert_ne!(a, b);
    }
}
