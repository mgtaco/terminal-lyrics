//! TTML → enhanced LRC, and the Spotify key extraction that finds it.

use terminal_lyrics::lrc;
use terminal_lyrics::lyrics::amll::spotify_track_id;
use terminal_lyrics::lyrics::ttml;

fn fixture(name: &str) -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    std::fs::read_to_string(format!("{path}{name}")).expect("fixture missing")
}

const SIMPLE: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml"
  xmlns:ttm="http://www.w3.org/ns/ttml#metadata"><body><div>
  <p begin="00:10.000" end="00:12.500">
    <span begin="00:10.000" end="00:10.600">Hello</span>
    <span begin="00:11.000" end="00:12.500">world</span>
    <span ttm:role="x-translation" xml:lang="zh-CN">你好世界</span>
  </p>
</div></body></tt>"#;

#[test]
fn word_spans_become_a2_tags_with_explicit_ends() {
    let a2 = ttml::to_enhanced_lrc(SIMPLE).unwrap();
    let parsed = lrc::parse(&a2);
    assert!(parsed.has_word_timings());

    let line = &parsed.lines[0];
    assert_eq!(line.text, "Hello world");
    assert_eq!(line.start, 10.0);

    let times: Vec<(f64, f64)> = line.words.iter().map(|w| (w.start, w.end)).collect();
    // The gap between 10.6 and 11.0 is real and must be preserved: without the
    // end tag the highlight would slide through the pause.
    assert_eq!(times, vec![(10.0, 10.6), (11.0, 12.5)]);
}

#[test]
fn translations_are_not_treated_as_lyrics() {
    let a2 = ttml::to_enhanced_lrc(SIMPLE).unwrap();
    assert!(!a2.contains('你'));
    let parsed = lrc::parse(&a2);
    assert_eq!(parsed.lines[0].text, "Hello world");
}

#[test]
fn background_vocals_are_left_out_of_the_main_line() {
    let xml = r#"<tt xmlns="http://www.w3.org/ns/ttml"
      xmlns:ttm="http://www.w3.org/ns/ttml#metadata"><body><div>
      <p begin="00:01.000" end="00:03.000">
        <span begin="00:01.000" end="00:01.500">Main</span>
        <span ttm:role="x-bg">
          <span begin="00:01.200" end="00:01.400">(echo)</span>
        </span>
        <span begin="00:02.000" end="00:03.000">line</span>
      </p>
    </div></body></tt>"#;
    let parsed = lrc::parse(&ttml::to_enhanced_lrc(xml).unwrap());
    assert_eq!(parsed.lines[0].text, "Main line");
    assert_eq!(parsed.lines[0].words.len(), 2);
}

#[test]
fn xml_entities_are_decoded() {
    let xml = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div>
      <p begin="00:01.000"><span begin="00:01.000" end="00:02.000">don&apos;t</span>
      <span begin="00:02.000" end="00:03.000">&amp;</span></p>
    </div></body></tt>"#;
    let parsed = lrc::parse(&ttml::to_enhanced_lrc(xml).unwrap());
    assert_eq!(parsed.lines[0].text, "don't &");
}

#[test]
fn a_real_database_entry_parses_end_to_end() {
    let a2 = ttml::to_enhanced_lrc(&fixture("wordtimed.ttml")).unwrap();
    let parsed = lrc::parse(&a2);
    assert!(parsed.has_word_timings());
    assert_eq!(parsed.lines.len(), 55);
    assert_eq!(parsed.lines[0].text, "Flashing Lights");
    assert_eq!(parsed.lines[0].words.len(), 2);

    // Every word must sit inside its line and inside the text it points at.
    for line in &parsed.lines {
        let chars = line.char_len();
        for w in &line.words {
            assert!(w.end >= w.start, "word ends before it starts");
            assert!(w.range.end <= chars, "word range escapes the line text");
            assert!(w.start >= line.start - 0.001, "word starts before its line");
        }
        for pair in line.words.windows(2) {
            assert!(pair[0].start <= pair[1].start, "words out of order");
        }
    }
}

#[test]
fn non_ttml_input_is_an_error_not_a_silent_empty_result() {
    assert!(ttml::to_enhanced_lrc("[00:10.00]this is an LRC file").is_err());
    assert!(ttml::to_enhanced_lrc("").is_err());
    // Well-formed XML that is not TTML.
    assert!(ttml::to_enhanced_lrc("<html><p>hi</p></html>").is_err());
}

#[test]
fn spotify_ids_are_recognised_in_every_form_the_players_use() {
    let want = "5TRPicyLGbAF2LGBFbHGvO";
    assert_eq!(
        spotify_track_id("https://open.spotify.com/track/5TRPicyLGbAF2LGBFbHGvO"),
        Some(want)
    );
    assert_eq!(
        spotify_track_id("https://open.spotify.com/track/5TRPicyLGbAF2LGBFbHGvO?si=abc123"),
        Some(want)
    );
    assert_eq!(spotify_track_id("spotify:track:5TRPicyLGbAF2LGBFbHGvO"), Some(want));
    assert_eq!(spotify_track_id(want), Some(want));
}

#[test]
fn non_spotify_keys_are_rejected_rather_than_fetched() {
    // These are what other players put in `xesam:url`; requesting a URL built
    // from one of them could only ever 404.
    assert_eq!(spotify_track_id("file:///home/me/music/song.mp3"), None);
    assert_eq!(spotify_track_id("/com/spotify/track/short"), None);
    assert_eq!(spotify_track_id("Artist\u{1}Title"), None);
    assert_eq!(spotify_track_id(""), None);
    // Right length, wrong alphabet.
    assert_eq!(spotify_track_id("!!!!!!!!!!!!!!!!!!!!!!"), None);
}
