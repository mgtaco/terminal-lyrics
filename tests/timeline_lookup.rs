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

const WORDED: &str = "[00:10.00]<00:10.00>one<00:10.50> <00:11.00>two<00:11.50> <00:12.00>three<00:12.90>\n[00:20.00]after\n";

#[test]
fn the_active_word_tracks_the_position() {
    let t = tl(WORDED);
    let word = |pos: f64| {
        t.active_word(0, pos).map(|r| {
            t.line(0)
                .unwrap()
                .text
                .chars()
                .skip(r.start)
                .take(r.end - r.start)
                .collect::<String>()
        })
    };
    assert_eq!(word(9.9), None, "nothing before the line starts");
    assert_eq!(word(10.0).as_deref(), Some("one"));
    assert_eq!(word(10.4).as_deref(), Some("one"));
    assert_eq!(word(11.0).as_deref(), Some("two"));
    assert_eq!(word(12.5).as_deref(), Some("three"));
}

#[test]
fn a_gap_between_words_holds_the_previous_one() {
    // "one" ends at 10.5 but "two" starts at 11.0. Blanking the screen through
    // that half second would read as a flicker, so the word is held.
    let t = tl(WORDED);
    let range = t.active_word(0, 10.75).expect("should hold a word");
    let text: String = t
        .line(0)
        .unwrap()
        .text
        .chars()
        .skip(range.start)
        .take(range.end - range.start)
        .collect();
    assert_eq!(text, "one");
}

#[test]
fn a_line_without_word_tags_has_no_active_word() {
    // The per-line fallback the display relies on: such a line must be shown
    // whole rather than disappearing.
    let t = tl(WORDED);
    assert!(t.line(1).unwrap().words.is_empty());
    assert_eq!(t.active_word(1, 21.0), None);
}

#[test]
fn the_highlight_offset_rebases_onto_the_active_word() {
    let t = tl(WORDED);
    // Midway through "two" (11.0..11.5): absolute offset sits inside 4..7.
    let abs = t.highlight_chars(0, 11.25);
    let range = t.active_word(0, 11.25).unwrap();
    assert_eq!(range, 4..7, "\"two\" occupies chars 4..7 of \"one two three\"");
    let relative = abs.saturating_sub(range.start);
    assert!(relative <= 3, "highlight must stay inside the word, got {relative}");
    assert!(relative >= 1, "and must have advanced into it, got {relative}");
}

const SYLLABLES: &str =
    "[00:10.00]<00:10.00>be<00:10.40>lieve<00:10.90> <00:11.00>it<00:11.40>\n[00:20.00]after\n";

fn shown(t: &Timeline, pos: f64) -> Option<String> {
    let r = t.active_word(0, pos)?;
    Some(
        t.line(0)
            .unwrap()
            .text
            .chars()
            .skip(r.start)
            .take(r.end - r.start)
            .collect(),
    )
}

#[test]
fn a_syllable_timed_word_builds_up_instead_of_being_replaced() {
    // The bug this pins: showing each timed span on its own puts "lieve" alone
    // on the screen, which is not a word.
    let t = tl(SYLLABLES);
    assert_eq!(shown(&t, 10.0).as_deref(), Some("be"));
    assert_eq!(shown(&t, 10.5).as_deref(), Some("believe"));
    // Held through the gap before the next word, still whole.
    assert_eq!(shown(&t, 10.95).as_deref(), Some("believe"));
    // A space ends the word: the next one starts from scratch.
    assert_eq!(shown(&t, 11.2).as_deref(), Some("it"));
}

#[test]
fn the_highlight_keeps_up_with_a_word_that_is_still_growing() {
    let t = tl(SYLLABLES);
    // Midway through "lieve" (10.40..10.90), which is chars 2..7 of the line.
    let range = t.active_word(0, 10.65).unwrap();
    assert_eq!(range, 0..7, "the whole word so far");
    let relative = t.highlight_chars(0, 10.65).saturating_sub(range.start);
    assert!(
        (3..=7).contains(&relative),
        "the highlight must be past \"be\" and inside the word, got {relative}"
    );
}
