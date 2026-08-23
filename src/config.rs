//! Configuration: defaults < config file < command-line flags.
//!
//! v1's central defect was options that were parsed and printed but never read.
//! The countermeasure here is structural: there is exactly one merge function,
//! [`Config::resolve`], every field flows through it, and `tests/config_layering.rs`
//! asserts per field that a flag beats the file and the file beats the default.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cli::Cli;

/// When to highlight individual words as they are sung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sweep {
    /// Highlight only when the source carries real per-word timestamps.
    /// Anything else would be highlighting a guess.
    Auto,
    /// Always highlight, interpolating across the phrase when the source is
    /// line-level.
    Always,
    /// Never highlight; show each line in one colour.
    Never,
}

impl Sweep {
    /// Whether to highlight, given what the loaded lyrics actually carry.
    pub fn applies(self, has_word_timings: bool) -> bool {
        match self {
            Sweep::Auto => has_word_timings,
            Sweep::Always => true,
            Sweep::Never => false,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sweep::Auto => "auto",
            Sweep::Always => "always",
            Sweep::Never => "never",
        }
    }

    /// Order used by the `s` key.
    pub fn next(self) -> Sweep {
        match self {
            Sweep::Auto => Sweep::Always,
            Sweep::Always => Sweep::Never,
            Sweep::Never => Sweep::Auto,
        }
    }
}

/// The settings the rest of the program reads. No `Option`s here: by this point
/// every value has been decided.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub player: Option<String>,
    pub font: String,
    pub offset_ms: i64,
    pub lrc_dir: Option<PathBuf>,
    pub network: bool,
    pub sweep: Sweep,
    /// Redraw interval. Only the sweep highlight moves between player events.
    pub tick_ms: u64,
    /// How far the predicted position may drift from the player's own
    /// `Position` before we re-anchor the clock.
    pub resync_threshold_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player: None,
            font: "block".to_string(),
            offset_ms: 0,
            lrc_dir: None,
            network: true,
            sweep: Sweep::Auto,
            tick_ms: 30,
            resync_threshold_ms: 250,
        }
    }
}

/// The on-disk form. Every field optional so a partial file is valid.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub player: Option<String>,
    pub font: Option<String>,
    pub offset_ms: Option<i64>,
    pub lrc_dir: Option<PathBuf>,
    pub network: Option<bool>,
    pub sweep: Option<Sweep>,
    pub tick_ms: Option<u64>,
    pub resync_threshold_ms: Option<u64>,
}

impl ConfigFile {
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("config file is not valid TOML")
    }

    /// Read a config file. A missing file is not an error; a malformed one is.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text)
                .with_context(|| format!("failed to read config {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("failed to read config {}", path.display())),
        }
    }
}

impl Config {
    /// The one and only merge point. Later sources win.
    pub fn resolve(file: ConfigFile, cli: &Cli) -> Self {
        let d = Config::default();
        Config {
            player: cli.player.clone().or(file.player).or(d.player),
            font: cli.font.clone().or(file.font).unwrap_or(d.font),
            offset_ms: cli.offset_ms.or(file.offset_ms).unwrap_or(d.offset_ms),
            lrc_dir: cli.lrc_dir.clone().or(file.lrc_dir).or(d.lrc_dir),
            // `--no-network` is a flag, so its absence means "defer to the file".
            network: if cli.no_network {
                false
            } else {
                file.network.unwrap_or(d.network)
            },
            sweep: cli
                .sweep_choice()
                .or(file.sweep)
                .unwrap_or(d.sweep),
            tick_ms: file.tick_ms.unwrap_or(d.tick_ms).clamp(10, 1000),
            resync_threshold_ms: file
                .resync_threshold_ms
                .unwrap_or(d.resync_threshold_ms)
                .clamp(50, 5000),
        }
    }
}

/// `$XDG_CONFIG_HOME/terminal-lyrics/config.toml`.
pub fn default_config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "terminal-lyrics")
        .map(|d| d.config_dir().join("config.toml"))
}

/// `$XDG_CACHE_HOME/terminal-lyrics/`.
pub fn cache_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "terminal-lyrics").map(|d| d.cache_dir().to_path_buf())
}
