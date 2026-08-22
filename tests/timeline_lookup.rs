//! Line lookup and sweep behaviour, especially at boundaries.

use terminal_lyrics::lrc;
use terminal_lyrics::timeline::{Position, Timeline};

fn tl(src: &str) -> Timeline {
    Timeline::new(lrc::parse(src))
}

const SONG: &str = "\
[00:10.00]first line
[00:20.00]second line
[00:30.00]third line
";

#[test]
fn before_the_first_line_is_the_intro() {
    let t = tl(SONG);
    assert_eq!(t.locate(0.0), Position::Intro { until: 10.0 });
    assert_eq!(t.locate(9.999), Position::Intro { until: 10.0 });
}

#[test]
fn a_line_starts_exactly_on_its_timestamp() {
    let t = tl(SONG);
    assert_eq!(t.locate(10.0), Position::Line { index: 0 });
    assert_eq!(t.locate(19.999), Position::Line { index: 0 });
    assert_eq!(t.locate(20.0), Position::Line { index: 1 });
}

#[test]
fn the_last_line_expires_instead_of_hanging_forever() {
    let t = tl(SONG);
    assert_eq!(t.locate(30.0), Position::Line { index: 2 });
    let end = t.line(2).unwrap().end;
    assert!(end.is_finite() && end > 30.0);
    assert_eq!(t.locate(end + 0.1), Position::Outro);
}

#[test]
fn empty_lyrics_never_panic() {
    let t = tl("");
    assert!(t.is_empty());
    assert_eq!(t.locate(0.0), Position::Outro);
    assert_eq!(t.highlight_chars(0, 5.0), 0);
    assert_eq!(t.highlight_chars(99, 5.0), 0);
}

#[test]
fn interpolated_sweep_runs_edge_to_edge() {
    let t = tl(SONG);
    let len = t.line(0).unwrap().char_len();
    assert_eq!(t.highlight_chars(0, 10.0), 0);
    assert_eq!(t.highlight_chars(0, 15.0), len / 2);
    assert_eq!(t.highlight_chars(0, 20.0), len);
    // Clamped outside the line rather than running off the end.
    assert_eq!(t.highlight_chars(0, 999.0), len);
    assert_eq!(t.highlight_chars(0, 0.0), 0);
}

#[test]
fn word_timings_drive_the_sweep_when_present() {
    let t = tl("[00:12.00]<00:12.00>Hello <00:12.50>there <00:13.20>world\n[00:20.00]next\n");
    assert!(t.lyrics().has_word_timings());

    // Start of "Hello".
    assert_eq!(t.highlight_chars(0, 12.0), 0);
    // "Hello" complete, "there" not yet started.
    assert_eq!(t.highlight_chars(0, 12.5), 6);
    // Midway through "there" (12.5..13.2): 6 + round(5 * 0.5) ≈ 8.
    let mid = t.highlight_chars(0, 12.85);
    assert!((7..=9).contains(&mid), "got {mid}");
    // Past the final word: whole line lit, never beyond its length.
    let len = t.line(0).unwrap().char_len();
    assert_eq!(t.highlight_chars(0, 19.9), len);
}

#[test]
fn blank_gap_lines_highlight_nothing() {
    let t = tl("[00:10.00]words here\n[00:12.00]\n[00:15.00]more words\n");
    let gap = 1;
    assert!(t.line(gap).unwrap().is_blank());
    assert_eq!(t.highlight_chars(gap, 13.0), 0);
}

#[test]
fn repeated_chorus_lines_are_located_independently() {
    let t = tl("[00:30.00][01:10.00]chorus\n[00:35.00]verse\n");
    assert_eq!(t.locate(31.0), Position::Line { index: 0 });
    assert_eq!(t.locate(36.0), Position::Line { index: 1 });
    assert_eq!(t.locate(71.0), Position::Line { index: 2 });
    assert_eq!(t.line(2).unwrap().text, "chorus");
}
