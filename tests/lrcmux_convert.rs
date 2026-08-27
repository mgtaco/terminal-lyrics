//! lrcmux JSON → enhanced LRC, against captured responses.
//!
//! Nothing here touches the network: the fixtures are real answers from
//! `api.lrcmux.dev`, saved so the conversion can be tested against the shapes
//! its upstreams actually emit rather than the shape the docs describe.

use terminal_lyrics::lrc;
use terminal_lyrics::lyrics::Source;
use terminal_lyrics::lyrics::lrcmux::{self, Response};

fn fixture(name: &str) -> Response {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    let text = std::fs::read_to_string(format!("{path}{name}")).expect("fixture missing");
    serde_json::from_str(&text).expect("fixture should deserialise")
}

#[test]
fn a_word_level_response_round_trips_with_word_timings() {
    let found = lrcmux::from_response(&fixture("lrcmux_word.json"), None).expect("a hit");
    assert!(found.lyrics.has_word_timings());
    assert!(found.synced);
    assert_eq!(
        found.source,
        Source::LrcMux {
            provider: "musixmatch".to_string()
        }
    );

    // The cache stores `raw` and re-parses it, so the round trip is the thing
    // that has to hold — not the intermediate string.
    let reparsed = lrc::parse(&found.raw);
    assert!(reparsed.has_word_timings());
    assert_eq!(reparsed.lines.len(), 3);
    assert_eq!(reparsed.lines[0].text, "When you were here before");
}

#[test]
fn words_are_separated_by_a_literal_space_not_run_together() {
    // The bug this file exists for. Musixmatch writes the separator *inside*
    // each word (`"When "`), and emitting it inside the tags too would leave
    // `<t>When <t>you` — no whitespace between the spans, which
    // `Line::continues_word` reads as one long word. The whole line would then
    // display as a single unbroken blob, and nothing else would look wrong.
    let found = lrcmux::from_response(&fixture("lrcmux_word.json"), None).expect("a hit");
    let line = &lrc::parse(&found.raw).lines[0];

    assert_eq!(line.words.len(), 5, "five words, one span each");
    for i in 1..line.words.len() {
        assert!(
            !line.continues_word(i),
            "word {i} of {:?} was read as a syllable of the one before it",
            line.text
        );
    }
    // And each word covers only itself.
    let first = &line.words[0];
    assert_eq!(&line.text[first.range.clone()], "When");
}

#[test]
fn milliseconds_become_seconds() {
    let found = lrcmux::from_response(&fixture("lrcmux_word.json"), None).expect("a hit");
    let line = &lrc::parse(&found.raw).lines[0];
    // start 19980 ms, first word 19980–20320 ms.
    assert!((line.start - 19.980).abs() < 1e-6, "got {}", line.start);
    assert!((line.words[0].start - 19.980).abs() < 1e-6);
    assert!((line.words[0].end - 20.320).abs() < 1e-6);
    // Written as mm:ss.SSS, the same form the TTML converter emits.
    assert!(found.raw.starts_with("[00:19.980]<00:19.980>When<00:20.320> "));
}

#[test]
fn a_gap_between_words_survives_as_a_real_gap() {
    // KuGou leaves 2.5s between "du" and the long "dudududududuud". Without
    // the closing tag the highlight would slide through that silence.
    let found = lrcmux::from_response(&fixture("lrcmux_tokens.json"), None).expect("a hit");
    let line = &lrc::parse(&found.raw).lines[0];
    let long = line
        .words
        .iter()
        .find(|w| &line.text[w.range.clone()] == "dudududududuud")
        .expect("the long word");
    assert!((long.start - 6.216).abs() < 1e-6);
    assert!((long.end - 7.146).abs() < 1e-6);
}

#[test]
fn a_whitespace_only_span_becomes_a_separator_not_a_word() {
    // KuGou times the space between words as a span of its own. Emitting a tag
    // for it would put a timed "word" made of nothing between every pair of
    // real ones — invisible on screen, and enough to make the word-by-word
    // display pause on empty.
    let found = lrcmux::from_response(&fixture("lrcmux_tokens.json"), None).expect("a hit");
    let parsed = lrc::parse(&found.raw);
    let line = &parsed.lines[0];

    assert_eq!(line.text, "Du, du dudududududuud,");
    assert!(
        line.words
            .iter()
            .all(|w| !line.text[w.range.clone()].trim().is_empty()),
        "a span covering only whitespace was kept: {:?}",
        line.words
    );

    // The comma butts straight up against "Du", so it reads as part of it —
    // which is right: it is not a word of its own to stop on.
    assert!(line.continues_word(1));
    // "du" follows a space, so it is.
    assert!(!line.continues_word(2));
}

#[test]
fn a_line_level_response_has_no_word_tags() {
    let found = lrcmux::from_response(&fixture("lrcmux_line.json"), None).expect("a hit");
    assert!(found.synced, "line level is still synced, just not per word");
    assert!(!found.lyrics.has_word_timings());
    assert!(
        !found.raw.contains('<'),
        "line-level output must not carry word tags: {:?}",
        found.raw
    );
    assert_eq!(found.raw, "[00:12.500]First line, no word tags\n[00:15.000]Second line\n");
    assert_eq!(
        found.source,
        Source::LrcMux {
            provider: "netease".to_string()
        }
    );
}

#[test]
fn an_instrumental_answer_is_a_miss_not_an_empty_hit() {
    // `level: "none"` says outright that there is nothing timed here. Falling
    // through lets LRCLIB offer plain text instead.
    assert!(lrcmux::from_response(&fixture("lrcmux_none.json"), None).is_none());
    assert!(lrcmux::to_enhanced_lrc(&fixture("lrcmux_none.json")).is_none());
}

#[test]
fn a_different_edit_of_the_song_is_rejected() {
    let resp = fixture("lrcmux_word.json"); // Creep, 238s
    assert!(lrcmux::from_response(&resp, Some(238.6)).is_some(), "the same edit");
    assert!(lrcmux::from_response(&resp, Some(242.0)).is_some(), "within 5s");
    assert!(
        lrcmux::from_response(&resp, Some(323.0)).is_none(),
        "a 323s edit's timings would be wrong from the first line to the last"
    );
    // Unknown length is not grounds for rejection; it is grounds for nothing.
    assert!(lrcmux::from_response(&resp, None).is_some());
}

#[test]
fn words_that_do_not_add_up_to_the_line_leave_the_line_intact() {
    // An upstream this code has not met could time only some of a line. The
    // text is what the user reads, so it survives whole, at line granularity.
    let resp: Response = serde_json::from_str(
        r#"{"meta": {"level": "word", "source": {"id": "genius"}},
            "lines": [{"text": "One two three", "start": 1000,
                       "words": [{"text": "One", "start": 1000, "end": 1500}]}]}"#,
    )
    .unwrap();
    let found = lrcmux::from_response(&resp, None).expect("a hit");
    assert_eq!(found.raw, "[00:01.000]One two three\n");
    assert_eq!(lrc::parse(&found.raw).lines[0].text, "One two three");
}
