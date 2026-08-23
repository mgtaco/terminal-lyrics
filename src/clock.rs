//! Playback position without polling.
//!
//! Polling a player process several times a second and inferring seeks from
//! position drift is the obvious approach and a bad one. Here the player tells
//! us where it is whenever it changes, we record that as an anchor, and
//! everything between anchors is interpolated from a monotonic `Instant`. A slow background poll exists only
//! to catch players (Spotify among them) that do not emit `Seeked` reliably.
//!
//! Every method takes `now` explicitly so the whole thing is testable without
//! sleeping.

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct SyncClock {
    anchor_pos: f64,
    anchor_at: Instant,
    rate: f64,
    playing: bool,
    /// User nudge in milliseconds; positive means "show lyrics later".
    offset_ms: i64,
}

impl SyncClock {
    pub fn new(offset_ms: i64, now: Instant) -> Self {
        Self {
            anchor_pos: 0.0,
            anchor_at: now,
            rate: 1.0,
            playing: false,
            offset_ms,
        }
    }

    /// Record a position reported by the player. This is the only way the clock
    /// learns the truth; everything else is interpolation from here.
    pub fn anchor(&mut self, pos: f64, rate: f64, playing: bool, now: Instant) {
        self.anchor_pos = pos.max(0.0);
        self.anchor_at = now;
        // A player reporting rate 0 while playing would freeze the lyrics.
        self.rate = if rate.is_finite() && rate > 0.0 {
            rate
        } else {
            1.0
        };
        self.playing = playing;
    }

    /// Pause/resume. Re-anchors first so the time already elapsed is kept.
    pub fn set_playing(&mut self, playing: bool, now: Instant) {
        if playing == self.playing {
            return;
        }
        let pos = self.raw_position(now);
        self.anchor(pos, self.rate, playing, now);
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn offset_ms(&self) -> i64 {
        self.offset_ms
    }

    pub fn set_offset_ms(&mut self, offset_ms: i64) {
        self.offset_ms = offset_ms.clamp(-30_000, 30_000);
    }

    pub fn nudge_offset_ms(&mut self, delta: i64) {
        self.set_offset_ms(self.offset_ms + delta);
    }

    /// Where the player itself would say it is, in seconds.
    pub fn raw_position(&self, now: Instant) -> f64 {
        if !self.playing {
            return self.anchor_pos;
        }
        let elapsed = now.saturating_duration_since(self.anchor_at).as_secs_f64();
        self.anchor_pos + elapsed * self.rate
    }

    /// Where to look in the lyrics: the player position, shifted by the nudge.
    pub fn lyric_position(&self, now: Instant) -> f64 {
        self.raw_position(now) - self.offset_ms as f64 / 1000.0
    }

    /// How far a freshly read player position differs from our prediction.
    pub fn drift(&self, reported: f64, now: Instant) -> Duration {
        let predicted = self.raw_position(now);
        Duration::from_secs_f64((reported - predicted).abs())
    }

    /// Re-anchor only if the player has moved somewhere we did not predict.
    /// Returns true when a correction was applied.
    pub fn reconcile(&mut self, reported: f64, threshold: Duration, now: Instant) -> bool {
        if self.drift(reported, now) <= threshold {
            return false;
        }
        self.anchor(reported, self.rate, self.playing, now);
        true
    }
}
