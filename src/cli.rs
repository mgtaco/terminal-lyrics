//! Command-line surface.
//!
//! Every option is `Option<T>` so that "not passed" is distinguishable from
//! "passed a value that happens to equal the default". `Config::resolve` is the
//! single place where defaults, the config file and these flags are merged.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::Sweep;
use crate::lyrics::Provider;

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

    /// Disable every network lookup and use only `--lrc-dir` and the cache.
    #[arg(long)]
    pub no_network: bool,

    /// Lyrics providers to consult, in order, comma-separated.
    /// Valid names: amll, lyricsplus, lrcmux, lrclib.
    #[arg(long, value_name = "LIST", value_delimiter = ',', value_parser = parse_provider)]
    pub providers: Option<Vec<Provider>>,

    /// Base URL of the LyricsPlus instance to query.
    #[arg(long, value_name = "URL")]
    pub lyricsplus_url: Option<String>,

    /// Base URL of the lrcmux instance to query.
    #[arg(long, value_name = "URL")]
    pub lrcmux_url: Option<String>,

    /// Always highlight words as they are sung, even when the timings are
    /// interpolated. By default the highlight appears only for lyrics that
    /// carry real per-word timestamps.
    #[arg(long, overrides_with = "no_sweep")]
    pub sweep: bool,

    /// Never highlight words; show each line in one colour.
    #[arg(long, overrides_with = "sweep")]
    pub no_sweep: bool,

    /// Show one word at a time when the lyrics carry real word timings.
    /// This is the default; `--whole-lines` turns it off.
    #[arg(long, overrides_with = "whole_lines")]
    pub word_by_word: bool,

    /// Always show the full lyric line, even when word timings are available.
    #[arg(long, overrides_with = "word_by_word")]
    pub whole_lines: bool,

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
        /// Spotify track ID or URL. When given, the word-timed AMLL database
        /// is tried first, exactly as it is for a live track.
        #[arg(long, value_name = "ID_OR_URL")]
        spotify_id: Option<String>,
    },
    /// Print the paths this build reads and writes.
    Paths,
}

/// One canonical provider-name parser, shared with the config file so a typo
/// gets the same message wherever it is written.
fn parse_provider(raw: &str) -> Result<Provider, String> {
    raw.parse()
}

impl Cli {
    /// `--word-by-word` / `--whole-lines` as an override, or `None` to defer
    /// to the config file and then the default.
    pub fn word_by_word_choice(&self) -> Option<bool> {
        match (self.word_by_word, self.whole_lines) {
            (true, false) => Some(true),
            (false, true) => Some(false),
            _ => None,
        }
    }

    /// `--sweep` / `--no-sweep` as an override, or `None` to defer to the
    /// config file and then the default.
    pub fn sweep_choice(&self) -> Option<Sweep> {
        match (self.sweep, self.no_sweep) {
            (true, false) => Some(Sweep::Always),
            (false, true) => Some(Sweep::Never),
            _ => None,
        }
    }
}
