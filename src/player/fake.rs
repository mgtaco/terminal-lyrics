//! A scripted player, so the sync engine can be tested without a bus.
//!
//! v1 had no way to exercise its sync logic except by playing music and
//! watching. Here a test writes a timeline of events and asserts on what the
//! clock believed at each step.

use std::time::{Duration, Instant};

use super::PlayerEvent;

/// An event plus how long after the script started it fires.
#[derive(Debug, Clone)]
pub struct Scripted {
    pub at: Duration,
    pub event: PlayerEvent,
}

/// Replays a fixed list of events against a caller-controlled clock.
#[derive(Debug, Clone)]
pub struct FakePlayer {
    script: Vec<Scripted>,
    next: usize,
    start: Instant,
}

impl FakePlayer {
    pub fn new(start: Instant, mut script: Vec<Scripted>) -> Self {
        script.sort_by_key(|s| s.at);
        Self {
            script,
            next: 0,
            start,
        }
    }

    /// Every event due at or before `now`, in order.
    pub fn drain_until(&mut self, now: Instant) -> Vec<PlayerEvent> {
        let elapsed = now.saturating_duration_since(self.start);
        let mut out = Vec::new();
        while let Some(s) = self.script.get(self.next) {
            if s.at > elapsed {
                break;
            }
            out.push(s.event.clone());
            self.next += 1;
        }
        out
    }

    pub fn is_finished(&self) -> bool {
        self.next >= self.script.len()
    }
}

/// Convenience for building scripts in tests.
pub fn at(ms: u64, event: PlayerEvent) -> Scripted {
    Scripted {
        at: Duration::from_millis(ms),
        event,
    }
}
