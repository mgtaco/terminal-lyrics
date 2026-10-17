//! The disk cache: one JSON file per track per provider, hits and misses both.
//!
//! The layout, the keys and the file contents are DankMaterialShell's, to the
//! byte, so the two programs can share one directory: whatever either of them
//! looked up, the other reads instead of asking again, and both show the same
//! lyrics for the same song. [`default_dir`] picks DMS's directory when it
//! exists.
//!
//! A miss is kept for 14 days. A line-synced hit older than 14 days is asked
//! for again once, and replaced only if word sync came back. Anything older
//! than 90 days is pruned, at most once a day.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::Provider;
use super::model::{Lyrics, Request, go_lower};

/// DMS's `cacheVersion`. A different number is a different cache.
pub const CACHE_VERSION: u32 = 4;
const NEGATIVE_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const UPGRADE_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const ENTRY_MAX_AGE: Duration = Duration::from_secs(90 * 24 * 60 * 60);
const PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const PRUNE_SENTINEL: &str = ".lastprune";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Entry {
    #[serde(rename = "v", default)]
    pub version: u32,
    #[serde(default)]
    pub fetched: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub miss: bool,
    #[serde(flatten)]
    pub lyrics: Lyrics,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn older_than(fetched: i64, age: Duration) -> bool {
    now() - fetched > age.as_secs() as i64
}

impl Entry {
    pub fn awaits_word_sync(&self) -> bool {
        !self.lyrics.synced.is_empty()
            && !self.lyrics.word_synced()
            && older_than(self.fetched, UPGRADE_TTL)
    }
}

/// Lowercase, runs of whitespace as one space, trimmed.
fn fold(s: &str) -> String {
    go_lower(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The track's key: the same for every provider, and for DMS.
pub fn track_key(req: &Request) -> String {
    let text = format!(
        "lyrics/v{CACHE_VERSION}\n{}\n{}\n{}\n{}",
        fold(&req.artist),
        fold(&req.title),
        fold(&req.album),
        req.seconds()
    );
    hex(&Sha256::digest(text.as_bytes()))
}

/// One provider's answer for the track. LRCLIB's is the bare track key, which
/// is where DMS's cache started.
pub fn provider_key(provider: Provider, req: &Request) -> String {
    let key = track_key(req);
    if provider == Provider::LrcLib {
        return key;
    }
    hex(&Sha256::digest(
        format!("{}\n{key}", provider.name()).as_bytes(),
    ))
}

/// DankMaterialShell's lyrics cache, when DMS is installed; otherwise our own.
pub fn default_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    let dms = base.cache_dir().join("DankMaterialShell");
    if dms.is_dir() {
        return Some(dms.join("lyrics"));
    }
    directories::ProjectDirs::from("", "", "terminal-lyrics").map(|d| d.cache_dir().join("lyrics"))
}

#[derive(Debug, Clone)]
pub struct Cache {
    dir: Option<PathBuf>,
}

impl Cache {
    /// `None` caches nothing, which every caller copes with.
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        Some(
            self.dir
                .as_ref()?
                .join(&key[..2])
                .join(format!("{key}.json")),
        )
    }

    /// A live entry for `key`: right version, and not an expired miss.
    pub fn load(&self, key: &str) -> Option<Entry> {
        let data = std::fs::read(self.path(key)?).ok()?;
        let entry: Entry = serde_json::from_slice(&data).ok()?;
        if entry.version != CACHE_VERSION {
            return None;
        }
        if entry.miss && older_than(entry.fetched, NEGATIVE_TTL) {
            return None;
        }
        Some(entry)
    }

    /// Stamp and write `entry`, atomically. A cache that cannot be written is
    /// not an error worth stopping for.
    pub fn store(&self, key: &str, entry: &mut Entry) {
        let Some(path) = self.path(key) else { return };
        entry.version = CACHE_VERSION;
        entry.fetched = now();
        let Ok(data) = serde_json::to_vec(entry) else {
            return;
        };
        let Some(dir) = path.parent() else { return };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let tmp = dir.join(format!(".tmp-{}-{}", std::process::id(), now_nanos()));
        if std::fs::write(&tmp, data).is_ok() && std::fs::rename(&tmp, &path).is_ok() {
            return;
        }
        let _ = std::fs::remove_file(&tmp);
    }

    pub fn remove(&self, key: &str) {
        if let Some(path) = self.path(key) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Remove entries older than 90 days, in the background, at most once a
    /// day — whichever of us or DMS gets there first.
    pub fn prune(&self) {
        let Some(root) = self.dir.clone() else { return };
        let sentinel = root.join(PRUNE_SENTINEL);
        if let Ok(modified) = std::fs::metadata(&sentinel).and_then(|m| m.modified())
            && modified.elapsed().is_ok_and(|age| age < PRUNE_INTERVAL)
        {
            return;
        }
        if std::fs::create_dir_all(&root).is_err() || std::fs::write(&sentinel, b"").is_err() {
            return;
        }
        std::thread::spawn(move || prune_older_than(&root, ENTRY_MAX_AGE));
    }
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn prune_older_than(dir: &Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            prune_older_than(&path, age);
        } else if path.extension().is_some_and(|e| e == "json")
            && meta
                .modified()
                .ok()
                .and_then(|m| m.elapsed().ok())
                .is_some_and(|elapsed| elapsed > age)
        {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_fold_case_and_spacing() {
        let a = Request {
            artist: "Daft  Punk".into(),
            title: "Get Lucky ".into(),
            duration: 248.4,
            ..Default::default()
        };
        let b = Request {
            artist: "daft punk".into(),
            title: "GET LUCKY".into(),
            duration: 248.0,
            ..Default::default()
        };
        assert_eq!(track_key(&a), track_key(&b));
        assert_ne!(provider_key(Provider::Unison, &a), track_key(&a));
        assert_eq!(provider_key(Provider::LrcLib, &a), track_key(&a));
    }

    #[test]
    fn reads_what_dms_writes() {
        let doc = r#"{"v":4,"fetched":1760000000,"source":"betterlyrics","synced":[{"t":31.4,"e":35.2,"x":"Like the legend","w":[{"t":31.4,"e":31.9,"x":"Like "},{"t":31.9,"x":"the legend"}],"voice":"v1"}],"plain":"Like the legend","instrumental":false,"voices":{"v1":{"name":"Lead","type":"person"}}}"#;
        let entry: Entry = serde_json::from_str(doc).unwrap();
        assert_eq!(entry.source, "betterlyrics");
        assert_eq!(entry.lyrics.synced[0].words.len(), 2);
        assert_eq!(entry.lyrics.voices["v1"].kind, "person");

        let miss: Entry = serde_json::from_str(
            r#"{"v":4,"fetched":1,"miss":true,"synced":null,"plain":"","instrumental":false}"#,
        )
        .unwrap();
        assert!(miss.miss && miss.lyrics.synced.is_empty());
    }

    #[test]
    fn store_then_load() {
        let dir = std::env::temp_dir().join(format!("tl-cache-{}", now_nanos()));
        let cache = Cache::new(Some(dir.clone()));
        let key = "ab".repeat(32);
        let mut entry = Entry {
            source: "lrclib".into(),
            ..Default::default()
        };
        entry.lyrics.plain = "hi".into();
        cache.store(&key, &mut entry);
        assert!(dir.join("ab").join(format!("{key}.json")).is_file());
        assert_eq!(cache.load(&key).unwrap().lyrics.plain, "hi");
        let _ = std::fs::remove_dir_all(dir);
    }
}
