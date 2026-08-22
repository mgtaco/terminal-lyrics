//! What the rest of the program knows about "something is playing".
//!
//! The TUI never talks to D-Bus directly: it consumes [`PlayerEvent`]s from a
//! channel. That seam is what lets `fake.rs` drive the whole sync engine in a
//! test with no bus, no player and no sleeping.

pub mod fake;
pub mod mpris;

use tokio::sync::mpsc;

/// The currently loaded track, normalised out of MPRIS metadata.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    /// Stable identity for cache keys: `xesam:url` when present (Spotify gives
    /// a permalink), otherwise `mpris:trackid`, otherwise artist+title.
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// Track length in seconds, when the player reports it.
    pub length: Option<f64>,
}

impl Track {
    pub fn is_usable(&self) -> bool {
        !self.title.trim().is_empty()
    }

    /// What the user sees while lyrics are being looked up.
    pub fn label(&self) -> String {
        if self.artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} — {}", self.artist, self.title)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// A different track was loaded (or the player cleared it).
    Track(Option<Box<Track>>),
    /// Play/pause. Carries the position the player reports at that moment.
    Status { playing: bool, position: f64 },
    /// The player jumped. Spotify does not send this reliably, hence `Tick`.
    Seeked { position: f64 },
    /// Result of the slow background `Position` read, used to correct drift.
    Tick { position: f64, playing: bool },
    /// The player vanished from the bus.
    Gone,
}

pub type EventRx = mpsc::UnboundedReceiver<PlayerEvent>;
pub type EventTx = mpsc::UnboundedSender<PlayerEvent>;
