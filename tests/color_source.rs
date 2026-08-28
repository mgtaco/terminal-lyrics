//! Parsing a colour source, and pulling an accent out of a palette.
//!
//! Everything here is pure: `accent()` itself touches the filesystem, so the
//! parts worth testing are the spec parser and the palette reader, both of which
//! take a `&str`.

use std::path::PathBuf;

use terminal_lyrics::config::{ColorSource, accent_from_palette, parse_hex};
use terminal_lyrics::render::{Theme, ThemeColor};

#[test]
fn the_default_is_the_terminals_own_palette() {
    // The whole point of the feature: setting no source must leave the display
    // following the terminal's colour scheme, exactly as it did before.
    assert_eq!(Theme::with_accent(None), Theme::default());
}

#[test]
fn an_accent_repaints_only_the_lyric_text() {
    let d = Theme::default();
    let themed = Theme::with_accent(Some([0xff, 0x66, 0x00]));
    assert_ne!(
        themed.unsung, d.unsung,
        "the accent belongs on the slot a line is actually drawn in"
    );
    assert_eq!(
        themed.sung, d.sung,
        "the sweep highlight stays a palette colour"
    );
    assert_eq!(themed.dim, d.dim, "so does the dimmed text");
}

#[test]
fn every_spec_form_parses() {
    assert_eq!("terminal".parse(), Ok(ColorSource::Terminal));
    assert_eq!("pywal".parse(), Ok(ColorSource::Pywal));
    assert_eq!(
        "fixed:#ff6600".parse(),
        Ok(ColorSource::Fixed([0xff, 0x66, 0x00]))
    );
    assert_eq!(
        "file:/tmp/colors.json".parse(),
        Ok(ColorSource::File(PathBuf::from("/tmp/colors.json")))
    );
}

#[test]
fn the_bare_names_are_case_insensitive_and_tolerate_padding() {
    assert_eq!("  TERMINAL  ".parse(), Ok(ColorSource::Terminal));
    assert_eq!("PyWal".parse(), Ok(ColorSource::Pywal));
}

#[test]
fn a_hash_is_optional_but_six_hex_digits_are_not() {
    assert_eq!(parse_hex("#ff6600"), Ok([0xff, 0x66, 0x00]));
    assert_eq!(parse_hex("ff6600"), Ok([0xff, 0x66, 0x00]));
    assert_eq!(parse_hex("FF6600"), Ok([0xff, 0x66, 0x00]));
    // Three-digit shorthand is not accepted, rather than being read as
    // something that happens to parse.
    assert!(parse_hex("#f60").is_err());
    assert!(parse_hex("#ff66000").is_err());
    assert!(parse_hex("#gggggg").is_err());
    assert!(parse_hex("").is_err());
}

#[test]
fn an_unknown_source_is_rejected_and_the_valid_ones_are_named() {
    // Same reasoning as an unknown provider: silently ignoring the typo would
    // look exactly like the colour source not working.
    let err = "pywall".parse::<ColorSource>().unwrap_err();
    assert!(
        err.contains("pywall"),
        "the typo should be quoted back: {err}"
    );
    assert!(
        err.contains("fixed:#rrggbb"),
        "and the valid forms named: {err}"
    );

    assert!("fixed:not-a-colour".parse::<ColorSource>().is_err());
    assert!("file:".parse::<ColorSource>().is_err());
    assert!("".parse::<ColorSource>().is_err());
}

#[test]
fn a_pywal_palette_yields_its_accent_slot() {
    // `color4` is the slot pywal's own templates use for highlights, so taking
    // it is what makes the lyrics match the rest of a themed desktop.
    // `r##` rather than `r#`: the palette is full of `"#rrggbb"`, and a `"#`
    // inside would close an `r#"` string early.
    let json = r##"{
        "special": {"background": "#1a1a1a", "foreground": "#eeeeee"},
        "colors": {"color0": "#1a1a1a", "color4": "#8a2be2", "color7": "#eeeeee"}
    }"##;
    assert_eq!(accent_from_palette(json), Some([0x8a, 0x2b, 0xe2]));
}

#[test]
fn a_palette_that_is_not_one_is_no_accent_rather_than_a_panic() {
    // pywal may simply never have run. That is a reason to keep the terminal
    // palette, not a reason to fail to start.
    assert_eq!(accent_from_palette("not json at all"), None);
    assert_eq!(accent_from_palette("{}"), None);
    assert_eq!(accent_from_palette(r#"{"colors": {}}"#), None);
    assert_eq!(
        accent_from_palette(r#"{"colors": {"color4": "nope"}}"#),
        None
    );
    assert_eq!(accent_from_palette(r#"{"colors": {"color4": 4}}"#), None);
}

#[test]
fn a_fixed_source_needs_no_filesystem_to_resolve() {
    assert_eq!(
        ColorSource::Fixed([1, 2, 3]).accent(),
        Some([1, 2, 3]),
        "a literal colour is already the answer"
    );
    assert_eq!(
        ColorSource::Terminal.accent(),
        None,
        "and the terminal source deliberately has no accent at all"
    );
}

#[test]
fn a_missing_palette_file_falls_back_rather_than_failing() {
    let missing = ColorSource::File(PathBuf::from("/nonexistent/colors.json"));
    assert_eq!(missing.accent(), None);
}

#[test]
fn the_label_round_trips_through_the_parser() {
    // `lyrics status` prints the label; it should be something you could paste
    // back in as a flag.
    for source in [
        ColorSource::Terminal,
        ColorSource::Pywal,
        ColorSource::Fixed([0xff, 0x66, 0x00]),
        ColorSource::File(PathBuf::from("/tmp/colors.json")),
    ] {
        let label = source.label();
        assert_eq!(
            label.parse::<ColorSource>(),
            Ok(source.clone()),
            "{label} should parse back to what printed it"
        );
    }
}

/// Every foreground colour actually used to draw a screen.
fn colours_drawn(screen: &terminal_lyrics::render::Screen<'_>, theme: Theme) -> Vec<ThemeColor> {
    let f = terminal_lyrics::render::font::block();
    let mut out: Vec<ThemeColor> = terminal_lyrics::render::render(screen, &f, 80, 24, theme)
        .lines
        .iter()
        .flat_map(|l| l.spans.iter())
        // Blank padding carries no colour worth counting.
        .filter(|s| !s.content.trim().is_empty())
        .filter_map(|s| s.style.fg)
        .collect();
    out.sort_by_key(|c| format!("{c:?}"));
    out.dedup();
    out
}

#[test]
fn the_accent_is_visible_with_the_sweep_off() {
    // The trap this exists for: `highlight` is 0 unless `sweep` applies, and
    // `sweep` defaults to "never" — so a line is drawn entirely in `unsung`
    // almost all of the time. An accent that only touched `sung` would be a
    // setting that parses, prints back, and changes nothing on screen.
    let accent = [0x8a, 0x2b, 0xe2];
    let screen = terminal_lyrics::render::Screen::Lyric {
        text: "HELLO",
        highlight: 0,
        reveal: 5,
        second: None,
    };
    let drawn = colours_drawn(&screen, Theme::with_accent(Some(accent)));
    assert!(
        drawn.contains(&ThemeColor::Rgb(accent[0], accent[1], accent[2])),
        "the accent never reached the screen; drew {drawn:?}"
    );
}

#[test]
fn without_an_accent_the_line_is_still_a_palette_colour() {
    // The default must keep following the terminal's own scheme, so nothing
    // drawn may be a fixed RGB value.
    let screen = terminal_lyrics::render::Screen::Lyric {
        text: "HELLO",
        highlight: 0,
        reveal: 5,
        second: None,
    };
    for colour in colours_drawn(&screen, Theme::default()) {
        assert!(
            !matches!(colour, ThemeColor::Rgb(..)),
            "{colour:?} is a fixed colour, not a palette entry"
        );
    }
}
