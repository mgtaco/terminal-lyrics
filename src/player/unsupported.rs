//! Placeholder for platforms with no backend yet.
//!
//! Its job is to keep the crate compiling and `cargo test` covering the pure
//! layer everywhere, so that adding a real backend is a new module and nothing
//! else. If this ever runs, it says so plainly rather than failing obscurely.
//!
//! # Windows
//!
//! The backend to write is `Windows.Media.Control`'s
//! `GlobalSystemMediaTransportControlsSessionManager`, which the `windows` crate
//! exposes. It supplies title, artist, album, playback status and a timeline
//! position, so `Snapshot` maps onto it cleanly, and `SourceAppUserModelId`
//! gives the player name. What it does not supply is a Spotify track ID, so
//! Windows would be LRCLIB line-level only — the word-by-word AMLL path needs
//! that ID and there is nowhere else to get it. Polling it on the same interval
//! as macOS is the simplest shape; the events it exposes are an optimisation,
//! not a requirement.

use std::time::Duration;

use anyhow::{Result, anyhow};

use super::{EventRx, PlayerState, Snapshot, Track};

fn unsupported<T>() -> Result<T> {
    Err(anyhow!(
        "no player backend for this platform yet — terminal-lyrics supports \
         Linux (MPRIS) and macOS (Spotify and Music)"
    ))
}

pub struct Session;

impl Session {
    pub async fn open() -> Result<Self> {
        unsupported()
    }

    pub async fn survey(&self) -> Result<Vec<PlayerState>> {
        unsupported()
    }

    pub async fn resolve(&self, _wanted: Option<&str>) -> Result<String> {
        unsupported()
    }

    pub async fn connect(&self, _name: &str) -> Result<PlayerHandle> {
        unsupported()
    }
}

pub struct PlayerHandle;

impl PlayerHandle {
    pub async fn snapshot(&self) -> Option<Snapshot> {
        None
    }

    pub async fn track(&self) -> Option<Track> {
        None
    }

    pub async fn position(&self) -> Option<f64> {
        None
    }

    pub async fn playing(&self) -> bool {
        false
    }

    pub async fn play_pause(&self) -> Result<()> {
        unsupported()
    }

    pub fn spawn(self, _poll_interval: Duration) -> (EventRx, tokio::task::JoinHandle<()>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(async move {
            let _ = tx.send(super::PlayerEvent::Gone);
        });
        (rx, handle)
    }
}
