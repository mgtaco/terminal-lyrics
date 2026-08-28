//! LyricsPlus: the JSON envelope, and the duration check that guards it.
//!
//! The fixture is a real `lyricsplus.prjktla.my.id` answer. The point of the
//! first test is that the document inside needs no special handling — it is
//! Apple's TTML, which the AMLL converter already speaks — so if this ever
//! stops holding, the envelope is not the only thing that changed.

use terminal_lyrics::lrc;
use terminal_lyrics::lyrics::{Source, lyricsplus, ttml};

fn ttml_fixture() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/lyricsplus.json"
    );
    let text = std::fs::read_to_string(path).expect("fixture missing");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["ttml"]
        .as_str()
        .expect("envelope has a ttml string")
        .to_string()
}

#[test]
fn apples_ttml_goes_straight_through_the_existing_converter() {
    let found = lyricsplus::from_ttml(&ttml_fixture(), None).expect("a hit");
    assert_eq!(found.source, Source::LyricsPlus);
    assert!(found.synced);
    assert!(found.lyrics.has_word_timings());
    assert!(found.lyrics.lines.len() > 20);

    let reparsed = lrc::parse(&found.raw);
    assert!(reparsed.has_word_timings());
    assert_eq!(reparsed.lines[0].text, "When you were here before");
}

#[test]
fn syllables_stay_syllables() {
    // The reason LyricsPlus is consulted before lrcmux: Apple times pieces of
    // a word, which is what the renderer builds a long word up out of. If this
    // ever came back all-words, the ordering would have lost its point.
    let found = lyricsplus::from_ttml(&ttml_fixture(), None).expect("a hit");
    let parsed = lrc::parse(&found.raw);
    let syllables = parsed
        .lines
        .iter()
        .any(|l| (1..l.words.len()).any(|i| l.continues_word(i)));
    assert!(syllables, "no line was timed in pieces");
}

#[test]
fn the_documents_own_duration_rejects_a_different_edit() {
    let xml = ttml_fixture();
    // Apple records `dur="3:58.640"`, which is Creep's length to the ms.
    let dur = ttml::document_duration(&xml).expect("a duration");
    assert!((dur - 238.640).abs() < 1e-3, "got {dur}");

    assert!(lyricsplus::from_ttml(&xml, Some(238.6)).is_some());
    assert!(lyricsplus::from_ttml(&xml, Some(300.0)).is_none());
    assert!(
        lyricsplus::from_ttml(&xml, None).is_some(),
        "no length, no opinion"
    );
}

#[test]
fn a_document_with_no_duration_is_not_rejected_for_it() {
    let xml = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div>
        <p begin="00:10.000" end="00:12.500">
          <span begin="00:10.000" end="00:10.600">Hello</span>
          <span begin="00:11.000" end="00:12.500">world</span>
        </p></div></body></tt>"#;
    assert!(ttml::document_duration(xml).is_none());
    assert!(lyricsplus::from_ttml(xml, Some(600.0)).is_some());
}

#[test]
fn junk_falls_through_instead_of_failing_the_lookup() {
    assert!(lyricsplus::from_ttml("not xml at all", None).is_none());
    assert!(lyricsplus::from_ttml("<tt><head/></tt>", None).is_none());
}
