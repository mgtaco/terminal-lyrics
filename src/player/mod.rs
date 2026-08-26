//! What the rest of the program knows about "something is playing".
//!
//! The TUI never talks to a media API directly: it consumes [`PlayerEvent`]s
//! from a channel. That seam is what lets `fake.rs` drive the whole sync engine
//! in a test with no player and no sleeping, and it is also where the supported
//! platforms diverge.
//!
//! # Backends
//!
//! Exactly one backend is compiled, chosen by target, and each exposes the same
//! `Session` and `PlayerHandle` items. There is no trait and no dynamic
//! dispatch, because there is never more than one implementation in the binary:
//!
//! | target | module | how it listens |
//! |---|---|---|
//! | Linux | [`mpris`] | D-Bus signals, plus a slow `Position` poll |
//! | macOS | [`macos`] | AppleScript, polled |
//! | other | [`unsupported`] | fails with a clear message |
//!
//! Everything above this line — [`Track`], [`PlayerEvent`], the ranking in
//! [`rank_players`] and the name matching in [`match_name`] — is
//! platform-neutral and stays testable everywhere.

pub mod fake;

#[cfg(target_os = "linux")]
pub mod mpris;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub mod unsupported;

#[cfg(target_os = "linux")]
use mpris as backend;
#[cfg(target_os = "macos")]
use macos as backend;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use unsupported as backend;

pub use backend::{PlayerHandle, Session};

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;

/// The currently loaded track, normalised out of whatever the platform reports.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    /// Stable identity for cache keys. Spotify gives a permalink or a
    /// `spotify:track:` URI either way, which is also the AMLL lookup key;
    /// other players fall back to their own id, then to artist+title.
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

/// The last-resort cache key, when the player offers no id of its own.
///
/// Shared so that a track keyed this way on one platform hits the same cache
/// entry on another.
pub fn fallback_id(artist: &str, title: &str) -> String {
    format!("{artist}\u{1}{title}")
}

/// Everything a backend can say about the player in one round trip.
///
/// The point is the round trip: on macOS each field would otherwise cost its own
/// subprocess, so the poll loop asks once and gets all three.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub playing: bool,
    pub position: f64,
    pub track: Option<Track>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// A different track was loaded (or the player cleared it).
    Track(Option<Box<Track>>),
    /// Play/pause. Carries the position the player reports at that moment.
    Status { playing: bool, position: f64 },
    /// The player jumped. Spotify does not send this reliably, hence `Tick`.
    Seeked { position: f64 },
    /// Result of the slow background position read, used to correct drift.
    Tick { position: f64, playing: bool },
    /// The player vanished.
    Gone,
}

pub type EventRx = mpsc::UnboundedReceiver<PlayerEvent>;
pub type EventTx = mpsc::UnboundedSender<PlayerEvent>;

/// What a candidate player looks like right now, for ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerState {
    pub name: String,
    pub playing: bool,
    pub has_track: bool,
}

/// Choose between players when the user has not named one.
///
/// Taking the first name alphabetically is wrong in practice: a browser
/// registers an idle `org.mpris.MediaPlayer2.chromium.instance…` that sorts
/// before `spotify` and reports no track at all, so the visualiser would follow
/// silence while music played next to it. Something actually playing wins.
pub fn rank_players(mut candidates: Vec<PlayerState>) -> Option<PlayerState> {
    fn score(p: &PlayerState) -> u8 {
        match (p.playing, p.has_track) {
            (true, true) => 3,
            (false, true) => 2,
            (true, false) => 1,
            (false, false) => 0,
        }
    }
    // Name as the tiebreak, so the choice is stable between runs.
    candidates.sort_by(|a, b| score(b).cmp(&score(a)).then_with(|| a.name.cmp(&b.name)));
    candidates.into_iter().next()
}

/// Resolve `--player` against the names that are actually available.
///
/// Exact match first, then prefix, both case-insensitively, so `--player spot`
/// finds `spotify` and `--player chromium` finds `chromium.instance26065`.
pub fn match_name(available: &[String], wanted: &str) -> Option<String> {
    let wanted = wanted.trim();
    available
        .iter()
        .find(|p| p.eq_ignore_ascii_case(wanted))
        .or_else(|| {
            let lower = wanted.to_lowercase();
            available.iter().find(|p| p.to_lowercase().starts_with(&lower))
        })
        .cloned()
}

/// Pick the player to follow, given a survey of what is running.
///
/// Shared by every backend so that `--player` behaves identically on all of
/// them. `what` names the thing that is missing, e.g. `"MPRIS player on the
/// session bus"`, because that part is the only bit that differs.
pub fn choose(states: Vec<PlayerState>, wanted: Option<&str>, what: &str) -> Result<String> {
    if states.is_empty() {
        return Err(anyhow!("no {what} is running — start a player and try again"));
    }
    match wanted {
        None => Ok(rank_players(states)
            .map(|p| p.name)
            .expect("a non-empty survey always ranks")),
        Some(w) => {
            let names: Vec<String> = states.into_iter().map(|p| p.name).collect();
            match_name(&names, w).ok_or_else(|| {
                anyhow!("no player matching {w:?}; available: {}", names.join(", "))
            })
        }
    }
}
