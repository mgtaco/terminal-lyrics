//! Defaults < config file < flags, asserted field by field.
//!
//! This file exists because of a specific v1 bug: `--no-split-commas` was
//! parsed, echoed back to the user as `Split commas: False`, and then ignored by
//! the code that did the splitting. Any option that stops being wired up should
//! fail here rather than in someone's terminal.

use std::path::PathBuf;

use clap::Parser;
use terminal_lyrics::cli::Cli;
use terminal_lyrics::config::{Config, ConfigFile, Sweep};

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
word_by_word = false
tick_ms = 77
resync_threshold_ms = 333
"#;

#[test]
fn defaults_apply_with_no_file_and_no_flags() {
    let cfg = resolve("", &[]);
    assert_eq!(cfg, Config::default());
    assert_eq!(cfg.font, "block");
    assert_eq!(cfg.offset_ms, 0);
    assert!(cfg.network);
    assert_eq!(cfg.sweep, Sweep::Never, "the highlight is opt-in");
    assert!(cfg.word_by_word, "word-timed lyrics show one word at a time");
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
    assert!(!cfg.word_by_word);
    assert_eq!(cfg.tick_ms, 77);
    assert_eq!(cfg.resync_threshold_ms, 333);
    // Nothing above silently matched the default and passed by luck.
    assert_ne!(cfg.font, d.font);
    assert_ne!(cfg.tick_ms, d.tick_ms);
    assert_ne!(cfg.sweep, d.sweep);
    assert_ne!(cfg.word_by_word, d.word_by_word);
    assert_ne!(cfg.network, d.network);
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
    // Zero is a real value, not "unset" — the `or`-on-falsy pattern that v1's
    // Python used would have thrown this away and fallen back to 111.
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
    assert_eq!(resolve(r#"sweep = "never""#, &["--sweep"]).sweep, Sweep::Always);
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
    assert!(Sweep::Auto.applies(true));
    assert!(!Sweep::Auto.applies(false));
    // The overrides ignore what the source carries.
    assert!(Sweep::Always.applies(false));
    assert!(Sweep::Always.applies(true));
    assert!(!Sweep::Never.applies(true));
    assert!(!Sweep::Never.applies(false));
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
