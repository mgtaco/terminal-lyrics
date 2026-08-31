//! Configuration: defaults < config file < command-line flags.
//!
//! The failure mode this guards against is an option that is parsed, echoed
//! back to the user, and then never read by the code it is supposed to control.
//! The countermeasure is structural: there is exactly one merge function,
//! [`Config::resolve`], every field flows through it, and `tests/config_layering.rs`
//! asserts per field that a flag beats the file and the file beats the default.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cli::Cli;
use crate::lyrics::lrcmux;
use crate::lyrics::{LRCMUX_URL, LYRICSPLUS_URL, Provider};

/// Where the accent colour comes from.
///
/// The default is deliberately not a colour at all. `Theme` is built from
/// terminal palette entries rather than fixed RGB precisely so the display
/// follows whatever scheme the terminal is set to, and a list of hardcoded
/// themes would throw that away. What this adds instead is a *source* for one
/// accent — the colour a lyric line is drawn in — leaving the rest of the
/// palette alone.
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

    /// The accent to paint the lyric text in, or `None` to leave the terminal
    /// palette alone.
    ///
    /// A palette that cannot be read falls back to `None` rather than failing:
    /// a malformed *spec* is the user's typo and is rejected at parse time, but
    /// a missing `colors.json` just means pywal has not run yet, and refusing to
    /// start over it would be a poor trade.
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

/// Pull the accent out of a pywal-shaped palette.
///
/// `color4` is pywal's own accent slot — the one its templates use for
/// highlights — so taking it is what makes `--color-source pywal` match the rest
/// of a themed desktop rather than merely being tinted by it.
pub fn accent_from_palette(text: &str) -> Option<[u8; 3]> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let hex = value.get("colors")?.get("color4")?.as_str()?;
    parse_hex(hex).ok()
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
    /// Highlight only when the source carries real per-word timestamps and
    /// the whole line is on screen. Anything else would be highlighting a
    /// guess, and with one word at a time there is nothing left to point at.
    Auto,
    /// Always highlight, interpolating across the phrase when the source is
    /// line-level.
    Always,
    /// Never highlight; show each line in one colour.
    Never,
}

impl Sweep {
    /// Whether to highlight, given what the loaded lyrics actually carry and
    /// how much of the line is on screen. `Auto` stays out of the way when the
    /// display is already down to one word: the sweep exists to say which word
    /// of the line is being sung, and a lone word says that by itself.
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
    /// Where a track that has never been nudged starts. A track the user has
    /// corrected with `,` / `.` keeps its own offset instead; see
    /// [`crate::offsets`].
    pub offset_ms: i64,
    pub lrc_dir: Option<PathBuf>,
    pub network: bool,
    pub sweep: Sweep,
    /// Where the accent colour comes from. `Terminal` leaves the palette alone.
    pub color_source: ColorSource,
    /// Show one word at a time when the lyrics carry real word timings.
    /// Ignored for line-level sources, which have nothing to split on.
    pub word_by_word: bool,
    /// Stack a second voice above the line when two are singing at once: a
    /// background vocal, or the other half of a duet. Off shows one line at a
    /// time, whichever came in most recently.
    pub overlapping_voices: bool,
    /// Redraw interval. Only the sweep highlight moves between player events.
    pub tick_ms: u64,
    /// How far the predicted position may drift from the player's own
    /// `Position` before we re-anchor the clock.
    pub resync_threshold_ms: u64,
    /// Which network sources to consult, in the order they are consulted.
    /// Dropping a name is how a provider is turned off.
    pub providers: Vec<Provider>,
    /// Base URL of the LyricsPlus instance. Overridable because the default is
    /// one person's server and the project documents self-hosting.
    pub lyricsplus_url: String,
    /// Base URL of the lrcmux instance, overridable for the same reason.
    pub lrcmux_url: String,
    /// Which of lrcmux's own upstreams it may answer from. lrcmux ranks them
    /// itself and cannot be told to prefer one, so this is a filter.
    pub lrcmux_sources: lrcmux::Sources,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player: None,
            font: "block".to_string(),
            offset_ms: 0,
            lrc_dir: None,
            network: true,
            sweep: Sweep::Never,
            color_source: ColorSource::Terminal,
            word_by_word: true,
            overlapping_voices: true,
            tick_ms: 30,
            resync_threshold_ms: 250,
            providers: Provider::DEFAULT_ORDER.to_vec(),
            lyricsplus_url: LYRICSPLUS_URL.to_string(),
            lrcmux_url: LRCMUX_URL.to_string(),
            lrcmux_sources: lrcmux::Sources::DEFAULT
                .parse()
                .expect("the built-in default parses"),
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
    pub color_source: Option<ColorSource>,
    pub word_by_word: Option<bool>,
    pub overlapping_voices: Option<bool>,
    pub tick_ms: Option<u64>,
    pub resync_threshold_ms: Option<u64>,
    /// An unknown name here is an error, not a silent skip: it would otherwise
    /// look exactly like the provider being unreachable.
    pub providers: Option<Vec<Provider>>,
    pub lyricsplus_url: Option<String>,
    pub lrcmux_url: Option<String>,
    /// A contradictory list here is an error for the same reason an unknown
    /// provider name is: silently ignored, it would look like the filter simply
    /// not working.
    pub lrcmux_sources: Option<lrcmux::Sources>,
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
            sweep: cli.sweep_choice().or(file.sweep).unwrap_or(d.sweep),
            color_source: cli
                .color_source
                .clone()
                .or(file.color_source)
                .unwrap_or(d.color_source),
            word_by_word: cli
                .word_by_word_choice()
                .or(file.word_by_word)
                .unwrap_or(d.word_by_word),
            overlapping_voices: cli
                .overlapping_voices_choice()
                .or(file.overlapping_voices)
                .unwrap_or(d.overlapping_voices),
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
            lyricsplus_url: cli
                .lyricsplus_url
                .clone()
                .or(file.lyricsplus_url)
                .unwrap_or(d.lyricsplus_url),
            lrcmux_url: cli
                .lrcmux_url
                .clone()
                .or(file.lrcmux_url)
                .unwrap_or(d.lrcmux_url),
            lrcmux_sources: cli
                .lrcmux_sources
                .clone()
                .or(file.lrcmux_sources)
                .unwrap_or(d.lrcmux_sources),
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

/// `$XDG_DATA_HOME/terminal-lyrics/offsets.json`, where the per-track sync
/// nudges are kept. Data rather than cache: they are the user's own
/// corrections, and a cache is by definition safe to delete.
pub fn offsets_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "terminal-lyrics")
        .map(|d| d.data_dir().join("offsets.json"))
}
