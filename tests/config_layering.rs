//! Defaults < config file < flags, asserted field by field.
//!
//! This file exists to catch one specific kind of bug: a flag that is parsed,
//! echoed back to the user, and then ignored by the code it is supposed to
//! control. Any option that stops being wired up should fail here rather than
//! in someone's terminal.

use std::path::PathBuf;

use clap::Parser;
use terminal_lyrics::cli::Cli;
use terminal_lyrics::config::{ColorSource, Config, ConfigFile, Sweep};
use terminal_lyrics::lyrics::Provider;

fn from_args(args: &[&str]) -> Cli {
    let mut full = vec!["lyrics"];
    full.extend_from_slice(args);
    Cli::try_parse_from(full).expect("args should parse")
}

fn resolve(file_toml: &str, args: &[&str]) -> Config {
    let file = ConfigFile::parse(file_toml).expect("fixture toml should parse");
    Config::resolve(file, &from_args(args))
}

const FULL_FILE: &str = r#"
player = "file-player"
font = "file-font"
offset_ms = 111
lrc_dir = "/file/lrc"
network = false
sweep = "always"
color_source = "fixed:#ff6600"
word_by_word = false
overlapping_voices = false
tick_ms = 77
resync_threshold_ms = 333
providers = ["lrcmux", "lrclib"]
lyricsplus_url = "https://lp.file"
lrcmux_url = "https://mux.file"
"#;

#[test]
fn defaults_apply_with_no_file_and_no_flags() {
    let cfg = resolve("", &[]);
    assert_eq!(cfg, Config::default());
    assert_eq!(cfg.font, "block");
    assert_eq!(cfg.offset_ms, 0);
    assert!(cfg.network);
    assert_eq!(cfg.sweep, Sweep::Never, "the highlight is opt-in");
    assert_eq!(
        cfg.color_source,
        ColorSource::Terminal,
        "the palette follows the terminal until told otherwise"
    );
    assert!(
        cfg.word_by_word,
        "word-timed lyrics show one word at a time"
    );
    assert!(
        cfg.overlapping_voices,
        "two voices at once are stacked, not hidden"
    );
    assert_eq!(
        cfg.providers,
        Provider::DEFAULT_ORDER.to_vec(),
        "all four sources, best-looking first"
    );
}

#[test]
fn file_overrides_every_default() {
    let cfg = resolve(FULL_FILE, &[]);
    let d = Config::default();
    assert_eq!(cfg.player.as_deref(), Some("file-player"));
    assert_eq!(cfg.font, "file-font");
    assert_eq!(cfg.offset_ms, 111);
    assert_eq!(cfg.lrc_dir, Some(PathBuf::from("/file/lrc")));
    assert!(!cfg.network);
    assert_eq!(cfg.sweep, Sweep::Always);
    assert_eq!(cfg.color_source, ColorSource::Fixed([0xff, 0x66, 0x00]));
    assert!(!cfg.word_by_word);
    assert!(!cfg.overlapping_voices);
    assert_eq!(cfg.tick_ms, 77);
    assert_eq!(cfg.resync_threshold_ms, 333);
    assert_eq!(cfg.providers, vec![Provider::LrcMux, Provider::LrcLib]);
    assert_eq!(cfg.lyricsplus_url, "https://lp.file");
    assert_eq!(cfg.lrcmux_url, "https://mux.file");
    // Nothing above silently matched the default and passed by luck.
    assert_ne!(cfg.font, d.font);
    assert_ne!(cfg.tick_ms, d.tick_ms);
    assert_ne!(cfg.sweep, d.sweep);
    assert_ne!(cfg.color_source, d.color_source);
    assert_ne!(cfg.word_by_word, d.word_by_word);
    assert_ne!(cfg.overlapping_voices, d.overlapping_voices);
    assert_ne!(cfg.network, d.network);
    assert_ne!(cfg.providers, d.providers);
    assert_ne!(cfg.lyricsplus_url, d.lyricsplus_url);
    assert_ne!(cfg.lrcmux_url, d.lrcmux_url);
}

#[test]
fn flag_beats_file_for_the_provider_list() {
    let cfg = resolve(FULL_FILE, &["--providers", "lrclib,amll"]);
    assert_eq!(cfg.providers, vec![Provider::LrcLib, Provider::Amll]);
    // The list is an order, not a set: the flag's order is the one used.
    assert_ne!(cfg.providers, vec![Provider::Amll, Provider::LrcLib]);
}

#[test]
fn flag_beats_file_for_both_provider_urls() {
    let cfg = resolve(
        FULL_FILE,
        &[
            "--lyricsplus-url",
            "http://lp.flag",
            "--lrcmux-url",
            "http://mux.flag",
        ],
    );
    assert_eq!(cfg.lyricsplus_url, "http://lp.flag");
    assert_eq!(cfg.lrcmux_url, "http://mux.flag");
}

#[test]
fn an_unknown_provider_name_is_rejected_and_the_valid_ones_are_named() {
    // A typo'd provider would otherwise be indistinguishable from that
    // provider simply never having anything to offer.
    let err = ConfigFile::parse(r#"providers = ["lrclub"]"#)
        .expect_err("a misspelled provider must not load");
    // `{:#}` so the TOML parser's own message, which carries ours, is included
    // rather than just the context line wrapped around it.
    let full = format!("{err:#}");
    assert!(
        full.contains("lrclub"),
        "the message names the typo: {full}"
    );
    for valid in ["amll", "lyricsplus", "lrcmux", "lrclib"] {
        assert!(full.contains(valid), "the message names {valid}: {full}");
    }

    assert!(Cli::try_parse_from(["lyrics", "--providers", "lrclub"]).is_err());
}

#[test]
fn an_empty_provider_list_is_allowed_and_means_no_network_sources() {
    // Equivalent to --no-network for the lookup, but leaves the cache alone.
    let cfg = resolve("providers = []", &[]);
    assert!(cfg.providers.is_empty());
}

#[test]
fn flag_beats_file_for_player() {
    let cfg = resolve(FULL_FILE, &["--player", "flag-player"]);
    assert_eq!(cfg.player.as_deref(), Some("flag-player"));
}

#[test]
fn flag_beats_file_for_font() {
    let cfg = resolve(FULL_FILE, &["--font", "flag-font"]);
    assert_eq!(cfg.font, "flag-font");
}

#[test]
fn flag_beats_file_for_offset_including_negatives_and_zero() {
    assert_eq!(resolve(FULL_FILE, &["--offset-ms", "-250"]).offset_ms, -250);
    // Zero is a real value, not "unset". An `or`-on-falsy merge would throw
    // this away and fall back to 111.
    assert_eq!(resolve(FULL_FILE, &["--offset-ms", "0"]).offset_ms, 0);
}

#[test]
fn flag_beats_file_for_lrc_dir() {
    let cfg = resolve(FULL_FILE, &["--lrc-dir", "/flag/lrc"]);
    assert_eq!(cfg.lrc_dir, Some(PathBuf::from("/flag/lrc")));
}

#[test]
fn no_network_flag_wins_and_its_absence_defers_to_the_file() {
    assert!(!resolve("network = true", &["--no-network"]).network);
    assert!(resolve("network = true", &[]).network);
    assert!(!resolve("network = false", &[]).network);
}

#[test]
fn sweep_flags_override_the_file() {
    // Flag on top of a file that says never.
    assert_eq!(
        resolve(r#"sweep = "never""#, &["--sweep"]).sweep,
        Sweep::Always
    );
    // Flag on top of a file that says always.
    assert_eq!(
        resolve(r#"sweep = "always""#, &["--no-sweep"]).sweep,
        Sweep::Never
    );
    // Neither flag: the file decides, including choosing auto explicitly.
    assert_eq!(resolve(r#"sweep = "never""#, &[]).sweep, Sweep::Never);
    assert_eq!(resolve(r#"sweep = "auto""#, &[]).sweep, Sweep::Auto);
    // Last flag wins if both are somehow passed.
    assert_eq!(resolve("", &["--sweep", "--no-sweep"]).sweep, Sweep::Never);
    assert_eq!(resolve("", &["--no-sweep", "--sweep"]).sweep, Sweep::Always);
}

#[test]
fn auto_highlights_only_lyrics_with_real_word_timings() {
    // The whole point of the default: a line-level LRC gets no moving
    // highlight, because there is nothing real to move it with.
    assert!(Sweep::Auto.applies(true, false));
    assert!(!Sweep::Auto.applies(false, false));
    // The overrides ignore what the source carries.
    assert!(Sweep::Always.applies(false, false));
    assert!(Sweep::Always.applies(true, false));
    assert!(!Sweep::Never.applies(true, false));
    assert!(!Sweep::Never.applies(false, false));
}

#[test]
fn auto_drops_the_highlight_when_only_one_word_is_on_screen() {
    // Nothing for the sweep to point at: the word on screen is the word being
    // sung. Asking for it outright still gets it.
    assert!(!Sweep::Auto.applies(true, true));
    assert!(Sweep::Always.applies(true, true));
    assert!(!Sweep::Never.applies(true, true));
    // Word-by-word is ignored for line-level lyrics, and so is this.
    assert!(!Sweep::Auto.applies(false, true));
}

#[test]
fn an_unknown_sweep_mode_is_rejected() {
    assert!(ConfigFile::parse(r#"sweep = "sometimes""#).is_err());
    // The old boolean form is a clear error, not a silent fallback.
    assert!(ConfigFile::parse("sweep = true").is_err());
}

#[test]
fn word_by_word_flags_override_the_file() {
    assert!(resolve("word_by_word = false", &["--word-by-word"]).word_by_word);
    assert!(!resolve("word_by_word = true", &["--whole-lines"]).word_by_word);
    assert!(!resolve("word_by_word = false", &[]).word_by_word);
    // Last flag wins.
    assert!(!resolve("", &["--word-by-word", "--whole-lines"]).word_by_word);
    assert!(resolve("", &["--whole-lines", "--word-by-word"]).word_by_word);
}

#[test]
fn overlapping_voice_flags_override_the_file() {
    let on = |c: terminal_lyrics::config::Config| c.overlapping_voices;
    assert!(on(resolve(
        "overlapping_voices = false",
        &["--overlapping-voices"]
    )));
    assert!(!on(resolve(
        "overlapping_voices = true",
        &["--single-voice"]
    )));
    assert!(!on(resolve("overlapping_voices = false", &[])));
    // Last flag wins.
    assert!(!on(resolve(
        "",
        &["--overlapping-voices", "--single-voice"]
    )));
    assert!(on(resolve("", &["--single-voice", "--overlapping-voices"])));
}

#[test]
fn the_two_display_settings_are_independent() {
    // One word at a time with no highlight is the default pairing; every other
    // combination must be reachable.
    let both = resolve("", &["--sweep", "--word-by-word"]);
    assert_eq!(both.sweep, Sweep::Always);
    assert!(both.word_by_word);

    let neither = resolve("", &["--no-sweep", "--whole-lines"]);
    assert_eq!(neither.sweep, Sweep::Never);
    assert!(!neither.word_by_word);

    let highlighted_lines = resolve("", &["--sweep", "--whole-lines"]);
    assert_eq!(highlighted_lines.sweep, Sweep::Always);
    assert!(!highlighted_lines.word_by_word);
}

#[test]
fn out_of_range_file_values_are_clamped_not_obeyed() {
    let cfg = resolve("tick_ms = 0\nresync_threshold_ms = 999999", &[]);
    assert_eq!(cfg.tick_ms, 10);
    assert_eq!(cfg.resync_threshold_ms, 5000);
}

#[test]
fn unknown_config_keys_are_an_error_not_a_silent_no_op() {
    // A typo'd key must not look like it worked.
    assert!(ConfigFile::parse("fnot = \"block\"").is_err());
}

#[test]
fn a_partial_file_leaves_other_fields_at_their_defaults() {
    let cfg = resolve("font = \"compact\"", &[]);
    assert_eq!(cfg.font, "compact");
    assert_eq!(cfg.offset_ms, Config::default().offset_ms);
    assert_eq!(cfg.tick_ms, Config::default().tick_ms);
}

#[test]
fn flag_beats_file_for_the_colour_source() {
    assert_eq!(
        resolve(
            r#"color_source = "pywal""#,
            &["--color-source", "fixed:#00ff00"]
        )
        .color_source,
        ColorSource::Fixed([0x00, 0xff, 0x00])
    );
    // And with no flag the file still wins over the default.
    assert_eq!(
        resolve(r#"color_source = "pywal""#, &[]).color_source,
        ColorSource::Pywal
    );
}

#[test]
fn an_unknown_colour_source_is_rejected_by_the_config_file_too() {
    // The flag and the file share one parser, so a typo fails the same way in
    // both rather than being silently ignored in one of them.
    assert!(ConfigFile::parse(r#"color_source = "pywall""#).is_err());
    assert!(ConfigFile::parse(r#"color_source = "fixed:#zzz""#).is_err());
    assert!(ConfigFile::parse("color_source = 4").is_err());
}
