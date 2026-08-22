//! Parser fixtures, including the two cases v1 got wrong.

use terminal_lyrics::lrc;

fn fixture(name: &str) -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    std::fs::read_to_string(format!("{path}{name}")).expect("fixture missing")
}

#[test]
fn accepts_timestamps_with_and_without_fractions() {
    // v1 regression: `parse_lrc_simple` required `\d{2}\.\d{1,3}` and silently
    // dropped `[00:10]`, so lines the processor emitted never reached the screen.
    let l = lrc::parse(&fixture("plain.lrc"));
    let starts: Vec<f64> = l.lines.iter().map(|x| x.start).collect();
    assert_eq!(starts, vec![10.0, 14.5, 18.25, 22.75]);
    assert_eq!(l.lines[0].text, "no decimal seconds");
}

#[test]
fn reads_id_tags_as_metadata_not_lyrics() {
    let l = lrc::parse(&fixture("plain.lrc"));
    assert_eq!(l.meta.title.as_deref(), Some("Test Song"));
    assert_eq!(l.meta.artist.as_deref(), Some("Test Artist"));
    assert_eq!(l.meta.album.as_deref(), Some("Test Album"));
    assert_eq!(l.meta.length, Some(200.0));
    // Tags and `#` comments must not become lyric lines.
    assert!(l.lines.iter().all(|x| !x.text.contains("Test Song")));
    assert_eq!(l.lines.len(), 4);
}

#[test]
fn expands_multi_timestamp_lines_in_order() {
    let l = lrc::parse(&fixture("multi.lrc"));
    let got: Vec<(f64, &str)> = l.lines.iter().map(|x| (x.start, x.text.as_str())).collect();
    assert_eq!(
        got,
        vec![
            (30.0, "repeated chorus line"),
            (35.0, "verse"),
            (70.0, "repeated chorus line"),
            (120.0, "repeated chorus line"),
        ]
    );
}

#[test]
fn parses_enhanced_word_tags() {
    let l = lrc::parse(&fixture("enhanced.lrc"));
    assert!(l.has_word_timings());

    let first = &l.lines[0];
    assert_eq!(first.text, "Hello there world");
    let starts: Vec<f64> = first.words.iter().map(|w| w.start).collect();
    assert_eq!(starts, vec![12.0, 12.5, 13.2]);

    // Ranges cover the words and stop before the separating spaces.
    let ranges: Vec<(usize, usize)> = first
        .words
        .iter()
        .map(|w| (w.range.start, w.range.end))
        .collect();
    assert_eq!(ranges, vec![(0, 5), (6, 11), (12, 17)]);

    // Each word ends where the next begins; the last ends with the line.
    assert_eq!(first.words[0].end, 12.5);
    assert_eq!(first.words[2].end, first.end);
}

#[test]
fn line_level_and_word_level_lines_coexist() {
    let l = lrc::parse(&fixture("enhanced.lrc"));
    let plain = l.lines.iter().find(|x| x.start == 15.0).unwrap();
    // No tags means no words — the timeline interpolates rather than the parser
    // inventing timings.
    assert!(plain.words.is_empty());
    assert_eq!(plain.text, "plain line without word tags");
}

#[test]
fn keeps_timestamped_blank_lines_as_gaps() {
    let l = lrc::parse(&fixture("enhanced.lrc"));
    let gap = l.lines.iter().find(|x| x.start == 18.0).unwrap();
    assert!(gap.is_blank());
    // The gap bounds the previous line, so the screen clears during the break.
    let before = l.lines.iter().find(|x| x.start == 15.0).unwrap();
    assert_eq!(before.end, 18.0);
}

#[test]
fn applies_offset_tag_to_every_timestamp() {
    let l = lrc::parse(&fixture("offset.lrc"));
    assert_eq!(l.meta.offset_ms, 500);
    assert_eq!(l.lines[0].start, 10.5);
    assert_eq!(l.lines[1].start, 20.5);
}

#[test]
fn skips_malformed_lines_without_losing_good_ones() {
    let l = lrc::parse(&fixture("messy.lrc"));
    let texts: Vec<&str> = l
        .lines
        .iter()
        .filter(|x| !x.is_blank())
        .map(|x| x.text.as_str())
        .collect();
    assert!(texts.contains(&"good line"));
    assert!(texts.contains(&"another good line"));
    assert!(texts.contains(&"comma fraction"));
    assert!(!texts.iter().any(|t| t.contains("no timestamp")));
    assert!(!texts.iter().any(|t| t.contains("not a time")));
}

#[test]
fn every_line_gets_a_finite_end() {
    for name in ["plain.lrc", "multi.lrc", "enhanced.lrc", "offset.lrc"] {
        let l = lrc::parse(&fixture(name));
        for (i, line) in l.lines.iter().enumerate() {
            assert!(line.end.is_finite(), "{name} line {i} has no end");
            assert!(line.end >= line.start, "{name} line {i} ends before it starts");
        }
    }
}

#[test]
fn empty_input_is_empty_not_a_panic() {
    let l = lrc::parse("");
    assert!(l.lines.is_empty());
    assert!(l.is_empty());
    assert!(!l.has_word_timings());
}
