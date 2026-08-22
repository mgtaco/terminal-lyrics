//! Command-line surface.
//!
//! Every option is `Option<T>` so that "not passed" is distinguishable from
//! "passed a value that happens to equal the default". `Config::resolve` is the
//! single place where defaults, the config file and these flags are merged.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Default)]
#[command(
    name = "lyrics",
    version,
    about = "Giant block lyrics in your terminal, synced to whatever is playing"
)]
pub struct Cli {
    /// Path to a config file (default: $XDG_CONFIG_HOME/terminal-lyrics/config.toml)
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// MPRIS player to follow, e.g. `spotify`. Default: first player found.
    #[arg(long, value_name = "NAME")]
    pub player: Option<String>,

    /// Font used for the big letters.
    #[arg(long, value_name = "NAME")]
    pub font: Option<String>,

    /// Shift lyrics in milliseconds; positive shows them later.
    #[arg(long, value_name = "MS", allow_negative_numbers = true)]
    pub offset_ms: Option<i64>,

    /// Look for `Artist - Title.lrc` here before querying the network.
    #[arg(long, value_name = "DIR")]
    pub lrc_dir: Option<PathBuf>,

    /// Disable the LRCLIB lookup and use only `--lrc-dir` and the cache.
    #[arg(long)]
    pub no_network: bool,

    /// Highlight words as they are sung (interpolated when the source is line-level).
    #[arg(long, overrides_with = "no_sweep")]
    pub sweep: bool,

    /// Show whole lines without a moving highlight.
    #[arg(long, overrides_with = "sweep")]
    pub no_sweep: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Print the resolved player, track and lyrics source, then exit.
    Status,
    /// Fetch lyrics for one track and print the LRC to stdout.
    Fetch {
        #[arg(long)]
        artist: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        album: Option<String>,
        /// Track length in seconds; greatly improves match accuracy.
        #[arg(long)]
        duration: Option<f64>,
    },
    /// Print the paths this build reads and writes.
    Paths,
}

impl Cli {
    /// `--sweep` / `--no-sweep` collapsed into one tri-state.
    pub fn sweep_choice(&self) -> Option<bool> {
        match (self.sweep, self.no_sweep) {
            (true, false) => Some(true),
            (false, true) => Some(false),
            _ => None,
        }
    }
}
