//! Defaults < config file < flags, all merged in [`Config::resolve`] alone, so
//! `tests/config_layering.rs` can catch an option that is parsed but never
//! read.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cli::Cli;
use crate::lyrics::Provider;
use crate::render::Case;

/// A source for the one accent colour, rather than hardcoded themes, so the
/// rest of the palette keeps following the terminal's scheme.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum ColorSource {
    /// The terminal's own palette, untouched.
    Terminal,
    /// A literal colour, `fixed:#rrggbb`.
    Fixed([u8; 3]),
    /// `~/.cache/wal/colors.json`, as written by pywal.
    Pywal,
    /// Any file in that same JSON shape, `file:PATH`.
    File(PathBuf),
}

impl ColorSource {
    pub fn label(&self) -> String {
        match self {
            ColorSource::Terminal => "terminal".to_string(),
            ColorSource::Fixed([r, g, b]) => format!("fixed:#{r:02x}{g:02x}{b:02x}"),
            ColorSource::Pywal => "pywal".to_string(),
            ColorSource::File(p) => format!("file:{}", p.display()),
        }
    }

    /// An unreadable palette falls back to `None`: a missing `colors.json` just
    /// means pywal has not run yet.
    pub fn accent(&self) -> Option<[u8; 3]> {
        match self {
            ColorSource::Terminal => None,
            ColorSource::Fixed(rgb) => Some(*rgb),
            ColorSource::Pywal => {
                // The user's own cache directory, not this program's: pywal
                // writes to `~/.cache/wal/` and knows nothing about us.
                let base = directories::BaseDirs::new()?;
                let path = base.cache_dir().join("wal").join("colors.json");
                accent_from_palette(&std::fs::read_to_string(path).ok()?)
            }
            ColorSource::File(path) => accent_from_palette(&std::fs::read_to_string(path).ok()?),
        }
    }
}

/// `#rrggbb`, or the same without the hash.
pub fn parse_hex(raw: &str) -> std::result::Result<[u8; 3], String> {
    let hex = raw.trim().strip_prefix('#').unwrap_or(raw.trim());
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("`{raw}` is not a #rrggbb colour"));
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).expect("checked hex digits above");
    Ok([byte(0), byte(2), byte(4)])
}

/// `color4` is pywal's accent slot, the one its templates use for highlights.
pub fn accent_from_palette(text: &str) -> Option<[u8; 3]> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let hex = value.get("colors")?.get("color4")?.as_str()?;
    parse_hex(hex).ok()
}

/// `dim_color` in the config file: the same `#rrggbb` the flag takes, and a
/// malformed one is an error at load rather than a colour that never appears.
fn deserialize_hex<'de, D>(d: D) -> std::result::Result<Option<[u8; 3]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(d)?;
    parse_hex(&raw).map(Some).map_err(serde::de::Error::custom)
}

impl std::str::FromStr for ColorSource {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let raw = s.trim();
        let lower = raw.to_ascii_lowercase();
        match lower.as_str() {
            "terminal" => return Ok(ColorSource::Terminal),
            "pywal" => return Ok(ColorSource::Pywal),
            _ => {}
        }
        if let Some(rest) = raw.strip_prefix("fixed:") {
            return parse_hex(rest).map(ColorSource::Fixed);
        }
        if let Some(rest) = raw.strip_prefix("file:") {
            let path = rest.trim();
            if path.is_empty() {
                return Err("`file:` needs a path after it".to_string());
            }
            return Ok(ColorSource::File(PathBuf::from(path)));
        }
        // Same reasoning as an unknown provider name: a silently ignored typo
        // here would look exactly like the colour source not working.
        Err(format!(
            "unknown colour source `{s}`; expected terminal, pywal, \
             fixed:#rrggbb or file:PATH"
        ))
    }
}

impl TryFrom<String> for ColorSource {
    type Error = String;

    fn try_from(s: String) -> std::result::Result<Self, String> {
        s.parse()
    }
}

/// When to highlight individual words as they are sung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sweep {
    /// Only with real per-word timestamps and the whole line on screen:
    /// anything else highlights a guess, or a lone word with nothing left to
    /// point at.
    Auto,
    /// Always highlight, interpolating across the phrase when the source is
    /// line-level.
    Always,
    /// Never highlight; show each line in one colour.
    Never,
}

impl Sweep {
    /// `Auto` stays out of the way at one word on screen: a lone word already
    /// says which word is being sung.
    pub fn applies(self, has_word_timings: bool, word_by_word: bool) -> bool {
        match self {
            Sweep::Auto => has_word_timings && !word_by_word,
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

    /// Order used by the `h` key.
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
    /// Upper or lower. Applied before layout, one character for one, so the
    /// sweep's offsets still line up.
    pub case: Case,
    /// Where a track that has never been nudged starts; see [`crate::offsets`].
    pub offset_ms: i64,
    pub lrc_dir: Option<PathBuf>,
    pub network: bool,
    pub sweep: Sweep,
    /// Where the accent colour comes from. `Terminal` leaves the palette alone.
    pub color_source: ColorSource,
    /// A literal colour for background vocals and the status lines. `None`
    /// draws them as the lyric colour with the terminal's faint attribute.
    pub dim_color: Option<[u8; 3]>,
    /// Show one word at a time when the lyrics carry real word timings.
    /// Ignored for line-level sources, which have nothing to split on.
    pub word_by_word: bool,
    /// Stack a second voice above the line when two sing at once; off shows the
    /// one that came in most recently.
    pub overlapping_voices: bool,
    /// Spectrum bars from `cava` under the lyrics; nothing is drawn when cava
    /// is not installed.
    pub visualizer: bool,
    /// Name the lyrics source in the bottom-left corner. Keypress notices are
    /// shown either way.
    pub show_source: bool,
    /// Redraw interval. Only the sweep highlight moves between player events.
    pub tick_ms: u64,
    /// How far the predicted position may drift from the player's own
    /// `Position` before we re-anchor the clock.
    pub resync_threshold_ms: u64,
    /// Which network sources to consult, in priority order. Dropping a name
    /// is how a provider is turned off.
    pub providers: Vec<Provider>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player: None,
            font: "block".to_string(),
            case: Case::Lower,
            offset_ms: 0,
            lrc_dir: None,
            network: true,
            sweep: Sweep::Auto,
            color_source: ColorSource::Terminal,
            dim_color: None,
            word_by_word: true,
            overlapping_voices: true,
            visualizer: false,
            show_source: true,
            tick_ms: 30,
            resync_threshold_ms: 250,
            providers: Provider::DEFAULT_ORDER.to_vec(),
        }
    }
}

/// The on-disk form. Every field optional so a partial file is valid.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub player: Option<String>,
    pub font: Option<String>,
    pub case: Option<Case>,
    pub offset_ms: Option<i64>,
    pub lrc_dir: Option<PathBuf>,
    pub network: Option<bool>,
    pub sweep: Option<Sweep>,
    pub color_source: Option<ColorSource>,
    #[serde(default, deserialize_with = "deserialize_hex")]
    pub dim_color: Option<[u8; 3]>,
    pub word_by_word: Option<bool>,
    pub overlapping_voices: Option<bool>,
    pub visualizer: Option<bool>,
    pub show_source: Option<bool>,
    pub tick_ms: Option<u64>,
    pub resync_threshold_ms: Option<u64>,
    /// An unknown name here is an error, not a silent skip: it would otherwise
    /// look exactly like the provider being unreachable.
    pub providers: Option<Vec<Provider>>,
    /// Settings for providers that are gone. Accepted and ignored, so a config
    /// written for an older version still loads.
    pub lyricsplus_url: Option<toml::Value>,
    pub lrcmux_url: Option<toml::Value>,
    pub lrcmux_sources: Option<toml::Value>,
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
            case: cli.case.or(file.case).unwrap_or(d.case),
            offset_ms: cli.offset_ms.or(file.offset_ms).unwrap_or(d.offset_ms),
            lrc_dir: cli.lrc_dir.clone().or(file.lrc_dir).or(d.lrc_dir),
            // `--no-network` is a flag, so its absence means "defer to the file".
            network: if cli.no_network {
                false
            } else {
                file.network.unwrap_or(d.network)
            },
            sweep: cli.sweep_choice().or(file.sweep).unwrap_or(d.sweep),
            color_source: cli
                .color_source
                .clone()
                .or(file.color_source)
                .unwrap_or(d.color_source),
            dim_color: cli.dim_color.or(file.dim_color).or(d.dim_color),
            word_by_word: cli
                .word_by_word_choice()
                .or(file.word_by_word)
                .unwrap_or(d.word_by_word),
            overlapping_voices: cli
                .overlapping_voices_choice()
                .or(file.overlapping_voices)
                .unwrap_or(d.overlapping_voices),
            visualizer: cli
                .visualizer_choice()
                .or(file.visualizer)
                .unwrap_or(d.visualizer),
            show_source: cli
                .show_source_choice()
                .or(file.show_source)
                .unwrap_or(d.show_source),
            tick_ms: file.tick_ms.unwrap_or(d.tick_ms).clamp(10, 1000),
            resync_threshold_ms: file
                .resync_threshold_ms
                .unwrap_or(d.resync_threshold_ms)
                .clamp(50, 5000),
            providers: cli
                .providers
                .clone()
                .or(file.providers)
                .unwrap_or(d.providers),
        }
    }
}

/// `$XDG_CONFIG_HOME/terminal-lyrics/config.toml`.
pub fn default_config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "terminal-lyrics")
        .map(|d| d.config_dir().join("config.toml"))
}

/// Where lyrics are cached: the shared cache when it exists, and
/// `$XDG_CACHE_HOME/terminal-lyrics/lyrics/` otherwise.
pub fn cache_dir() -> Option<PathBuf> {
    crate::lyrics::cache::default_dir()
}

/// Data rather than cache: the nudges are the user's own corrections, and a
/// cache is safe to delete.
pub fn offsets_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "terminal-lyrics")
        .map(|d| d.data_dir().join("offsets.json"))
}
