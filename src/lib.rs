//! terminal-lyrics: giant block lyrics in the terminal, synced over MPRIS.
//!
//! Everything lives behind a library target so the pure parts — parser,
//! timeline, clock, config layering, layout — are testable without a terminal
//! or a running player. Without that seam there is nowhere to put a test.
// An option that stops being read should break the build rather than quietly
// stop working. Dead flags are silent for a long time otherwise.
#![warn(unused, dead_code, unreachable_pub)]


pub mod clock;
pub mod cli;
pub mod config;
pub mod lrc;
pub mod lyrics;
pub mod offsets;
pub mod player;
pub mod render;
pub mod sync;
pub mod timeline;
pub mod tui;
