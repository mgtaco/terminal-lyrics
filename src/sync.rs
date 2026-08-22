//! Player events in, a believable playback position out.
//!
//! Kept separate from the TUI so it can be driven by [`crate::player::fake`] in
//! a test. The rule is simple: the player is always right when it speaks, and
//! between utterances we interpolate. The `Tick` arm is the interesting one —
//! it re-anchors only on real divergence, so a player that reports position a
//! few milliseconds off does not cause a visible jump every second.

use std::time::{Duration, Instant};

use crate::clock::SyncClock;
use crate::player::{PlayerEvent, Track};

/// What the caller needs to react to, above just redrawing.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// A different track is loaded; the caller should fetch lyrics.
    Track(Option<Box<Track>>),
    /// The position moved somewhere unpredicted.
    Resynced,
    /// The player disappeared.
    Gone,
    /// Nothing the caller must act on.
    None,
}

pub struct SyncEngine {
    clock: SyncClock,
    track: Option<Track>,
    threshold: Duration,
}

impl SyncEngine {
    pub fn new(offset_ms: i64, threshold: Duration, now: Instant) -> Self {
        Self {
            clock: SyncClock::new(offset_ms, now),
            track: None,
            threshold,
        }
    }

    pub fn clock(&self) -> &SyncClock {
        &self.clock
    }

    pub fn clock_mut(&mut self) -> &mut SyncClock {
        &mut self.clock
    }

    pub fn track(&self) -> Option<&Track> {
        self.track.as_ref()
    }

    /// Position to look up in the lyrics, including the user's offset nudge.
    pub fn lyric_position(&self, now: Instant) -> f64 {
        self.clock.lyric_position(now)
    }

    pub fn is_playing(&self) -> bool {
        self.clock.is_playing()
    }

    pub fn apply(&mut self, event: PlayerEvent, now: Instant) -> Change {
        match event {
            PlayerEvent::Track(next) => {
                let next = next.map(|b| *b);
                // MPRIS players re-emit metadata for all sorts of reasons
                // (art loaded, rating changed). Only a real identity change
                // should throw away the lyrics we already have.
                let same = match (&self.track, &next) {
                    (Some(a), Some(b)) => a.id == b.id,
                    (None, None) => true,
                    _ => false,
                };
                self.track = next.clone();
                if same {
                    Change::None
                } else {
                    Change::Track(next.map(Box::new))
                }
            }
            PlayerEvent::Status { playing, position } => {
                self.clock.anchor(position, 1.0, playing, now);
                Change::Resynced
            }
            PlayerEvent::Seeked { position } => {
                self.clock.anchor(position, 1.0, self.clock.is_playing(), now);
                Change::Resynced
            }
            PlayerEvent::Tick { position, playing } => {
                if playing != self.clock.is_playing() {
                    self.clock.anchor(position, 1.0, playing, now);
                    return Change::Resynced;
                }
                if self.clock.reconcile(position, self.threshold, now) {
                    Change::Resynced
                } else {
                    Change::None
                }
            }
            PlayerEvent::Gone => {
                self.track = None;
                Change::Gone
            }
        }
    }
}
