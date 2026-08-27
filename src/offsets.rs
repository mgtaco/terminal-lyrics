//! Per-track sync offsets, remembered between runs.
//!
//! A badly timed file is badly timed for that one song, so nudging with `,` and
//! `.` is a property of the track, not of the session. Carrying the nudge into
//! the next song was the old behaviour and it was always wrong: the next song
//! is timed fine, and the correction that rescued the last one now breaks it.
//!
//! So the offset is stored against the track's cache key and reapplied when the
//! track comes back. A song that has never been tuned has no entry and starts
//! at `offset_ms` from the config — which is what that key now means: the
//! starting point for a track nobody has corrected yet.
//!
//! Only tuned tracks are stored. Resetting one with `0` deletes its entry
//! rather than writing a zero, so the file stays a list of corrections rather
//! than growing an entry per song ever played.
//!
//! Disk is the truth on every read and every write: the file is a few hundred
//! bytes and is touched only on a track change or a keypress, so re-reading it
//! costs nothing and means two `lyrics` running at once cannot silently drop
//! each other's corrections. It is written pretty-printed and keyed in sorted
//! order because it is small enough to be worth editing by hand.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Same range the clock clamps to; a stored value outside it could otherwise
/// come back as something the keys could never have produced.
const LIMIT_MS: i64 = 30_000;

const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    offset_ms: i64,
    /// "Artist — Title", so the file can be read and edited by a human.
    label: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    version: u32,
    /// Keyed by [`crate::player::Track::id`] — the same key the lyrics cache
    /// uses, so a track that resolves to one cache entry has one offset.
    tracks: BTreeMap<String, Entry>,
}

/// The saved corrections. A store with nowhere to write still works for the
/// length of the session; it just forgets on exit.
#[derive(Debug)]
pub struct Offsets {
    path: Option<PathBuf>,
    /// Used only when there is no path, so there is exactly one source of
    /// truth at a time.
    memory: BTreeMap<String, Entry>,
}

impl Offsets {
    /// A store that cannot create its directory degrades to memory rather than
    /// failing the run — losing the nudges is better than losing the lyrics.
    pub fn new(path: Option<PathBuf>) -> Self {
        if let Some(p) = &path
            && let Some(dir) = p.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            debug(&format!("offsets not saved ({}): {e}", dir.display()));
            return Self {
                path: None,
                memory: BTreeMap::new(),
            };
        }
        Self {
            path,
            memory: BTreeMap::new(),
        }
    }

    /// Nowhere to write to; nudges last only for this session.
    pub fn in_memory() -> Self {
        Self::new(None)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The saved offset for a track, if it has ever been tuned.
    pub fn get(&self, key: &str) -> Option<i64> {
        self.read().tracks.get(key).map(|e| e.offset_ms)
    }

    /// What the clock should be set to for this track: its own saved offset,
    /// or the configured starting point when it has none.
    pub fn offset_for(&self, key: &str, default_ms: i64) -> i64 {
        self.get(key).unwrap_or(default_ms)
    }

    /// Remember a correction. An empty key means the player told us nothing
    /// stable enough to key on, so there is nothing to remember it against.
    pub fn set(&mut self, key: &str, label: &str, offset_ms: i64) {
        if key.is_empty() {
            return;
        }
        let mut tracks = self.read().tracks;
        tracks.insert(
            key.to_string(),
            Entry {
                offset_ms: offset_ms.clamp(-LIMIT_MS, LIMIT_MS),
                label: label.to_string(),
            },
        );
        self.commit(tracks);
    }

    /// Forget one track's correction, so it starts from the config default
    /// again. Absence is how "not tuned" is spelled.
    pub fn clear(&mut self, key: &str) {
        let mut tracks = self.read().tracks;
        if tracks.remove(key).is_some() {
            self.commit(tracks);
        }
    }

    /// How many tracks have been tuned. Reported by `lyrics paths`.
    pub fn len(&self) -> usize {
        self.read().tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whatever is on disk right now. Any unreadable or unrecognised file
    /// reads as empty: the corrections are a convenience, and refusing to
    /// start over a corrupt one would be the worse failure.
    fn read(&self) -> Saved {
        let Some(path) = &self.path else {
            return Saved {
                version: VERSION,
                tracks: self.memory.clone(),
            };
        };
        let file: Saved = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
            .unwrap_or_default();
        if file.version != VERSION {
            return Saved::default();
        }
        file
    }

    fn commit(&mut self, tracks: BTreeMap<String, Entry>) {
        let Some(path) = self.path.clone() else {
            self.memory = tracks;
            return;
        };
        let file = Saved {
            version: VERSION,
            tracks,
        };
        let Ok(json) = serde_json::to_string_pretty(&file) else {
            return;
        };
        // Write-then-rename, as the cache does: a process killed mid-write
        // must not leave a truncated file that then reads as no corrections
        // at all.
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
