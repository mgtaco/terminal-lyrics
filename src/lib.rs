//! terminal-lyrics: giant block lyrics in the terminal, synced over MPRIS.
//!
//! Everything lives behind a library target so the pure parts — parser,
//! timeline, clock, config layering, layout — are testable without a terminal
//! or a running player. v1 had no seam like this and consequently no tests.
// An option that stops being read should break the build rather than quietly
// stop working — which is how v1's dead flags survived for so long.
#![warn(unused, dead_code, unreachable_pub)]


pub mod clock;
pub mod cli;
pub mod config;
pub mod lrc;
pub mod lyrics;
pub mod player;
pub mod render;
pub mod sync;
pub mod timeline;
pub mod tui;
