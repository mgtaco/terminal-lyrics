//! Per-track sync offsets, one for each source the track's lyrics have come
//! from: two providers rarely share a timing error. Only tuned pairs are
//! stored, and the file is re-read on every write so two running copies agree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Same range the clock clamps to; a stored value outside it could otherwise
/// come back as something the keys could never have produced.
const LIMIT_MS: i64 = 30_000;

/// Version 1 kept one offset per track, tuned against whichever provider was
/// showing at the time; there is no telling which, so those are not carried
/// over.
const VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    /// "Artist — Title", so the file can be read and edited by a human.
    label: String,
    /// Keyed by [`crate::lyrics::Source::sync_key`].
    offsets: BTreeMap<String, i64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    version: u32,
    /// Keyed by [`crate::player::Track::id`].
    tracks: BTreeMap<String, Entry>,
}

/// A store with nowhere to write forgets on exit.
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

    pub fn in_memory() -> Self {
        Self::new(None)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The saved offset for a track's lyrics from `source`, if it has ever
    /// been tuned.
    pub fn get(&self, track: &str, source: &str) -> Option<i64> {
        self.read().tracks.get(track)?.offsets.get(source).copied()
    }

    pub fn offset_for(&self, track: &str, source: &str, default_ms: i64) -> i64 {
        self.get(track, source).unwrap_or(default_ms)
    }

    /// An empty track key means the player gave nothing stable to remember it
    /// against.
    pub fn set(&mut self, track: &str, source: &str, label: &str, offset_ms: i64) {
        if track.is_empty() || source.is_empty() {
            return;
        }
        let mut tracks = self.read().tracks;
        let entry = tracks.entry(track.to_string()).or_insert_with(|| Entry {
            label: String::new(),
            offsets: BTreeMap::new(),
        });
        entry.label = label.to_string();
        entry
            .offsets
            .insert(source.to_string(), offset_ms.clamp(-LIMIT_MS, LIMIT_MS));
        self.commit(tracks);
    }

    /// Absence is how "not tuned" is spelled, so the track starts from the
    /// default.
    pub fn clear(&mut self, track: &str, source: &str) {
        let mut tracks = self.read().tracks;
        let Some(entry) = tracks.get_mut(track) else {
            return;
        };
        if entry.offsets.remove(source).is_none() {
            return;
        }
        if entry.offsets.is_empty() {
            tracks.remove(track);
        }
        self.commit(tracks);
    }

    /// How many tracks have been tuned. Reported by `lyrics paths`.
    pub fn len(&self) -> usize {
        self.read().tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// An unreadable file reads as empty: refusing to start over a corrupt one
    /// would be the worse failure.
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
        // Write-then-rename: a process killed mid-write must not leave a
        // truncated file that reads as no corrections.
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
