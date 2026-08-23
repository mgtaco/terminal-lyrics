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
    assert_eq!(parsed.lines.len(), 6);
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

#[test]
fn bare_seconds_are_a_valid_time_not_a_dropped_line() {
    // The bug this guards: AMLL writes `4.658` below a minute and `1:04.579`
    // above it, in the same file. Requiring a colon silently dropped every
    // line in the first minute, so songs appeared to start at the second verse.
    assert_eq!(ttml::parse_time("4.658"), Some(4.658));
    assert_eq!(ttml::parse_time("1:04.579"), Some(64.579));
    assert_eq!(ttml::parse_time("00:04.658"), Some(4.658));
    assert_eq!(ttml::parse_time("01:02:03.500"), Some(3723.5));
}

#[test]
fn offset_times_with_metric_suffixes_are_understood() {
    assert_eq!(ttml::parse_time("4.658s"), Some(4.658));
    assert_eq!(ttml::parse_time("250ms"), Some(0.25));
    assert_eq!(ttml::parse_time("2m"), Some(120.0));
    assert_eq!(ttml::parse_time("1h"), Some(3600.0));
    // Frames and ticks need a frame rate these files do not carry; guessing
    // would put words at the wrong moment.
    assert_eq!(ttml::parse_time("10f"), None);
    assert_eq!(ttml::parse_time("10t"), None);
    assert_eq!(ttml::parse_time(""), None);
    assert_eq!(ttml::parse_time("soon"), None);
}

#[test]
fn a_file_mixing_both_time_formats_keeps_every_line() {
    let a2 = ttml::to_enhanced_lrc(&fixture("mixed_timeformat.ttml")).unwrap();
    let parsed = lrc::parse(&a2);
    // The fixture is trimmed to nine lines that straddle the one-minute mark:
    // five written as bare seconds, four with a colon. Before the fix the five
    // were silently dropped.
    assert_eq!(parsed.lines.len(), 9, "every line must survive");
    assert_eq!(
        parsed.lines.iter().filter(|l| l.start < 60.0).count(),
        5,
        "the bare-second lines are the ones that used to vanish"
    );
    assert_eq!(parsed.lines.iter().filter(|l| l.start >= 60.0).count(), 4);
    // The first verse, which used to vanish entirely.
    assert_eq!(parsed.lines[0].text, "I threw a wish in the well");
    assert!((parsed.lines[0].start - 4.658).abs() < 0.001);
    assert!(parsed.has_word_timings());

    // The song must start at its beginning, not a minute in.
    assert!(parsed.lines[0].start < 10.0, "the opening line is missing");
}

#[test]
fn an_unparsable_time_fails_loudly_instead_of_dropping_the_line() {
    // Better to fall back to LRCLIB's complete line-level lyrics than to show
    // a song with a hole in it.
    let xml = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div>
      <p begin="00:01.000"><span begin="00:01.000" end="00:02.000">fine</span></p>
      <p begin="17f"><span begin="17f" end="18f">broken</span></p>
    </div></body></tt>"#;
    let err = ttml::to_enhanced_lrc(xml).unwrap_err().to_string();
    assert!(err.contains("partial"), "unhelpful error: {err}");
}
