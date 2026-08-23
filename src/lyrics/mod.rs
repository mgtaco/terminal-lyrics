//! Finding lyrics for a track.
//!
//! Order: local `--lrc-dir` file, then cache (positive *and* negative), then
//! LRCLIB. Everything is async and runs off the UI task, so a slow network
//! never freezes the display.

pub mod amll;
pub mod cache;
pub mod lrclib;
pub mod normalize;
pub mod ttml;

use std::path::{Path, PathBuf};

use crate::lrc::{self, Lyrics};
use crate::player::Track;

/// Where a set of lyrics came from — shown by `lyrics status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    LocalFile(PathBuf),
    Cache,
    LrcLib { id: i64 },
    /// The AMLL TTML database — the word-timed source.
    Amll,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::LocalFile(p) => write!(f, "file {}", p.display()),
            Source::Cache => write!(f, "cache"),
            Source::LrcLib { id } => write!(f, "lrclib #{id}"),
            Source::Amll => write!(f, "amll"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Found {
    pub lyrics: Lyrics,
    pub source: Source,
    /// False when only unsynced plain text was available.
    pub synced: bool,
    /// The document exactly as received. The cache stores *this*, not
    /// `lyrics.plain_text()` — round-tripping through plain text would drop
    /// every timestamp and silently unsync the lyrics on the next hit.
    pub raw: String,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Found(Box<Found>),
    /// The track is known to have no lyrics (or is instrumental).
    Missing,
}

/// `Artist - Title.lrc` next to the user's own files, checked before the network.
pub fn local_lookup(dir: &Path, track: &Track) -> Option<Found> {
    let stem = if track.artist.is_empty() {
        track.title.clone()
    } else {
        format!("{} - {}", track.artist, track.title)
    };
    for ext in ["lrc", "txt"] {
        let path = dir.join(format!("{stem}.{ext}"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let parsed = lrc::parse(&text);
        if parsed.is_empty() {
            continue;
        }
        let synced = parsed.lines.iter().any(|l| l.start > 0.0);
        return Some(Found {
            lyrics: parsed,
            source: Source::LocalFile(path),
            synced,
            raw: text,
        });
    }
    None
}
