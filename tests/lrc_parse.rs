//! Parser fixtures, including the cases that are easiest to get wrong.

use terminal_lyrics::lrc;

fn fixture(name: &str) -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    std::fs::read_to_string(format!("{path}{name}")).expect("fixture missing")
}

#[test]
fn accepts_timestamps_with_and_without_fractions() {
    // A parser requiring `\d{2}\.\d{1,3}` silently drops `[00:10]`, so whole
    // lines never reach the screen while the file looks perfectly valid.
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

#[test]
fn a_trailing_word_tag_closes_the_previous_word() {
    // A2 end tags. Without them a real pause between words is invisible and
    // the highlight slides through it.
    let l = lrc::parse("[00:10.00]<00:10.00>Flashing<00:10.60> <00:11.00>Lights<00:12.50>\n[00:20.00]next\n");
    let words = &l.lines[0].words;
    assert_eq!(words.len(), 2, "an end tag must not become a word");
    assert_eq!(l.lines[0].text, "Flashing Lights");
    assert_eq!((words[0].start, words[0].end), (10.0, 10.6));
    assert_eq!((words[1].start, words[1].end), (11.0, 12.5));
}

#[test]
fn words_without_end_tags_still_run_to_the_next_one() {
    let l = lrc::parse("[00:10.00]<00:10.00>one <00:11.00>two\n[00:15.00]next\n");
    let words = &l.lines[0].words;
    assert_eq!((words[0].start, words[0].end), (10.0, 11.0));
    // The last word runs to the end of its line.
    assert_eq!(words[1].end, l.lines[0].end);
}

#[test]
fn adjacent_word_tags_are_syllables_of_one_word() {
    // Word-timed sources time a long word in pieces. The pieces are butted
    // straight together in the text; only whitespace separates real words.
    let l = lrc::parse("[00:10.00]<00:10.00>be<00:10.40>lieve<00:10.90> <00:11.00>it<00:11.40>\n[00:15.00]next\n");
    let line = &l.lines[0];
    assert_eq!(line.text, "believe it");
    assert_eq!(line.words.len(), 3, "three timed spans, two words");

    assert!(!line.continues_word(0), "the first span opens a word");
    assert!(line.continues_word(1), "\"lieve\" continues \"be\"");
    assert!(!line.continues_word(2), "a space starts a new word");

    assert_eq!(line.word_bounds(0), Some(0..7), "\"believe\"");
    assert_eq!(line.word_bounds(1), Some(0..7));
    assert_eq!(line.word_bounds(2), Some(8..10), "\"it\"");
    assert_eq!(line.word_bounds(3), None);
}
