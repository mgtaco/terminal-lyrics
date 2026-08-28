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
            assert!(
                line.end >= line.start,
                "{name} line {i} ends before it starts"
            );
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
    let l = lrc::parse(
        "[00:10.00]<00:10.00>Flashing<00:10.60> <00:11.00>Lights<00:12.50>\n[00:20.00]next\n",
    );
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
    let l = lrc::parse(
        "[00:10.00]<00:10.00>be<00:10.40>lieve<00:10.90> <00:11.00>it<00:11.40>\n[00:15.00]next\n",
    );
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

#[test]
fn a_background_marker_is_a_second_voice_not_body_text() {
    let l = lrc::parse(
        "[00:10.000][end:00:14.000]I will always love you\n\
         [00:11.000][bg:00:10.000][end:00:13.000](ooh ooh)\n",
    );
    assert_eq!(l.lines.len(), 1, "a backing vocal is not a line of its own");
    assert_eq!(l.lines[0].text, "I will always love you");
    let second = &l.lines[0].secondary[0];
    assert_eq!(second.text, "(ooh ooh)");
    assert!(second.background);
    assert!((second.start - 11.0).abs() < 0.001);
    assert!((second.end - 13.0).abs() < 0.001);
}

#[test]
fn an_unknown_bracket_group_is_still_body_text() {
    // The marker slot sits where a bracket group that is not a clock already
    // stopped the timestamp loop. It must not start swallowing the annotations
    // people really do write at the head of a line.
    let l = lrc::parse("[00:10.000][verse 1]words\n");
    assert_eq!(l.lines[0].text, "[verse 1]words");
}

#[test]
fn word_ranges_are_measured_after_the_markers() {
    // The markers are consumed before the body is read, so a word's char range
    // indexes the text and nothing else. Getting this wrong shifts every
    // highlight by the width of the markers.
    let plain = lrc::parse("[00:10.000]<00:10.000>Hello <00:11.000>world\n");
    let marked = lrc::parse("[00:10.000][end:00:14.000]<00:10.000>Hello <00:11.000>world\n");
    assert_eq!(plain.lines[0].text, marked.lines[0].text);
    assert_eq!(
        plain.lines[0]
            .words
            .iter()
            .map(|w| w.range.clone())
            .collect::<Vec<_>>(),
        marked.lines[0]
            .words
            .iter()
            .map(|w| w.range.clone())
            .collect::<Vec<_>>(),
    );
    assert_eq!(marked.lines[0].words[1].range, 6..11);
}

#[test]
fn an_overlap_too_short_to_be_a_duet_is_still_sequential() {
    // A third of a second is one line's tail running into the next one's head,
    // which is what most overlap in the wild is. It must read exactly as it did
    // before second voices existed.
    let l =
        lrc::parse("[00:10.000][end:00:14.300]first line\n[00:14.000][end:00:18.000]second line\n");
    assert!(l.lines[1].secondary.is_empty());
    assert!((l.lines[0].end - l.lines[1].start).abs() < 0.001);
}

#[test]
fn a_long_overlap_puts_the_first_voice_above_the_second() {
    // Two and a half seconds of the four are sung together: both voices, not
    // one voice finishing as the other starts.
    let l = lrc::parse(
        "[00:10.000][end:00:14.000]first voice\n[00:11.500][end:00:16.000]second voice\n",
    );
    // The earlier line keeps its own window right up to the moment the second
    // one arrives: it is only a second voice once there is something to be
    // second to.
    assert!(l.lines[0].secondary.is_empty());
    assert!((l.lines[0].end - 11.5).abs() < 0.001);

    let second = &l.lines[1].secondary[0];
    assert_eq!(second.text, "first voice");
    assert!(!second.background, "a duet partner is not a backing vocal");
    assert!((second.start - 11.5).abs() < 0.001);
    assert!((second.end - 14.0).abs() < 0.001);
}

#[test]
fn two_lines_at_the_same_timestamp_do_not_leave_one_unreachable() {
    // The earlier one used to get `end == start`, which no lookup can ever
    // land on, so it was invisible however long you stared at it.
    let l = lrc::parse("[00:10.000]first voice\n[00:10.000]second voice\n[00:20.000]after\n");
    assert_eq!(
        l.lines.len(),
        2,
        "the two are one line and its second voice"
    );
    assert_eq!(l.lines[0].text, "second voice");
    assert_eq!(l.lines[0].secondary[0].text, "first voice");
    assert!(l.lines[0].end > l.lines[0].start, "and it has a window");
}

#[test]
fn a_repeated_line_at_one_timestamp_is_not_stacked_on_itself() {
    let l = lrc::parse("[00:10.000]same words\n[00:10.000]same words\n[00:20.000]after\n");
    assert_eq!(l.lines.len(), 2);
    assert!(
        l.lines[0].secondary.is_empty(),
        "that is a duplicate, not a duet"
    );
}

#[test]
fn the_offset_tag_moves_word_ends_as_well_as_word_starts() {
    // Shifting the starts and leaving the ends behind stretches every word by
    // the offset, and the highlight smears across the gaps between them.
    let l = lrc::parse(
        "[offset:+500]\n[00:10.000]<00:10.000>Hello<00:11.000> <00:12.000>world<00:13.000>\n",
    );
    let w = &l.lines[0].words[0];
    assert!((w.start - 10.5).abs() < 0.001);
    assert!((w.end - 11.5).abs() < 0.001, "the end moved too");
}

#[test]
fn a_line_whose_only_word_tags_are_background_is_not_word_timed() {
    // `has_word_timings` ends the provider search and keeps a cache entry
    // forever. A backing vocal must not be able to flip either.
    let l = lrc::parse(
        "[00:10.000]a line with no timings\n\
         [00:11.000][bg:00:10.000][end:00:13.000]<00:11.000>(ooh)<00:13.000>\n",
    );
    assert!(!l.lines[0].secondary.is_empty(), "the voice is still there");
    assert!(!l.has_word_timings());
}

#[test]
fn a_second_voice_survives_being_reparsed_from_its_own_text() {
    // Everything reaches the screen by way of the cache, which stores the LRC
    // text and parses it again. Anything not written down here is lost on the
    // second play and nowhere else.
    let text = "[00:10.000][end:00:14.000]I will always love you\n\
                [00:11.000][bg:00:10.000][end:00:13.000](ooh ooh)\n";
    assert_eq!(lrc::parse(text), lrc::parse(&format!("{text}{}", "")));
    let once = lrc::parse(text);
    assert_eq!(once.lines[0].secondary[0].text, "(ooh ooh)");
}

#[test]
fn a_brief_overlap_between_two_long_lines_is_still_slop() {
    // Over a second, so the duration rule alone would let it through — but it
    // is a small part of either phrase, which is what a tail running long looks
    // like on lines this length.
    let l = lrc::parse(
        "[00:10.000][end:00:21.200]a long first line\n\
         [00:20.000][end:00:31.000]a long second line\n",
    );
    assert!(l.lines[1].secondary.is_empty());
}
