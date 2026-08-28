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
fn background_vocals_become_a_second_voice_above_the_line() {
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
    // The backing vocal is a voice of its own, so it stays out of the line's
    // own text and words — it must not interleave with what is being read.
    assert_eq!(parsed.lines[0].text, "Main line");
    assert_eq!(parsed.lines[0].words.len(), 2);

    let second = &parsed.lines[0].secondary[0];
    assert_eq!(second.text, "(echo)");
    assert!(second.background, "an x-bg span is a background vocal");
    assert!((second.start - 1.2).abs() < 0.001);
    assert!((second.end - 1.4).abs() < 0.001);
}

#[test]
fn a_translation_inside_a_background_span_is_still_dropped() {
    // The shape the real database writes: the translation of the backing vocal
    // lives inside the backing vocal. Capturing one must not capture the other.
    let parsed = lrc::parse(&ttml::to_enhanced_lrc(&fixture("duet_bg.ttml")).unwrap());
    let second = &parsed.lines[0].secondary[0];
    assert_eq!(second.text, "(Know)");
    assert!(
        !parsed.lines.iter().any(|l| l.text.contains('知')),
        "translations are still not lyrics, wherever they are nested"
    );
}

#[test]
fn a_background_wrapper_with_no_times_takes_them_from_its_spans() {
    let parsed = lrc::parse(&ttml::to_enhanced_lrc(&fixture("duet_bg.ttml")).unwrap());
    let second = &parsed.lines[1].secondary[0];
    assert_eq!(second.text, "(ooh)");
    assert!(
        (second.start - 21.0).abs() < 0.001,
        "start came from the span"
    );
    assert!((second.end - 22.0).abs() < 0.001, "end came from the span");
}

#[test]
fn a_background_line_is_attached_to_its_own_line_not_the_nearest_one() {
    // This one comes in at 34 s, after the following line has already started
    // at 33 s. Time order alone would hang it on the wrong line.
    let parsed = lrc::parse(&ttml::to_enhanced_lrc(&fixture("duet_bg.ttml")).unwrap());
    let host = parsed
        .lines
        .iter()
        .find(|l| l.text == "Long third line")
        .expect("the line the background belongs to");
    assert_eq!(host.secondary[0].text, "(late)");
    let neighbour = parsed
        .lines
        .iter()
        .find(|l| l.text == "Fourth line")
        .expect("the line it starts over");
    assert!(
        neighbour.secondary.iter().all(|s| !s.background),
        "the background belongs to the line that declared it"
    );
}

#[test]
fn a_paragraph_end_is_written_out_instead_of_discarded() {
    // Without it there is no way to tell a real overlap from a line that simply
    // runs until the next one starts.
    let a2 = ttml::to_enhanced_lrc(&fixture("duet_bg.ttml")).unwrap();
    assert!(a2.contains("[end:"), "got: {a2}");
}

#[test]
fn a_quarter_second_overlap_is_slop_not_a_duet() {
    // The median overlap across the database is a quarter of a second: one
    // phrase's tail running into the next one's head. Stacking those would put
    // a second line on screen a couple of times a song for a few frames each.
    let parsed = lrc::parse(&ttml::to_enhanced_lrc(&fixture("duet_bg.ttml")).unwrap());
    let after = parsed
        .lines
        .iter()
        .find(|l| l.text == "Slop after")
        .expect("the second half of the near-miss pair");
    assert!(after.secondary.is_empty(), "got: {:?}", after.secondary);
}

#[test]
fn two_voices_overlapping_for_seconds_are_both_kept() {
    let parsed = lrc::parse(&ttml::to_enhanced_lrc(&fixture("duet_bg.ttml")).unwrap());
    let later = parsed
        .lines
        .iter()
        .find(|l| l.text == "And I will always need you")
        .expect("the voice that comes in second");
    let second = &later.secondary[0];
    assert_eq!(second.text, "I will always love you");
    assert!(!second.background, "a duet partner is not a backing vocal");
}

#[test]
fn a_background_vocal_with_an_unreadable_time_is_dropped_rather_than_failing_the_file() {
    // The opposite call to the one made for a <p>: a line nobody can place in
    // time still fails loudly, because partial lyrics are worse than none. A
    // backing vocal is texture, and losing the whole song over it would not be.
    let xml = r#"<tt xmlns="http://www.w3.org/ns/ttml"
      xmlns:ttm="http://www.w3.org/ns/ttml#metadata"><body><div>
      <p begin="00:01.000" end="00:03.000">
        <span begin="00:01.000" end="00:03.000">Main line</span>
        <span ttm:role="x-bg" begin="12f" end="14f"><span>(echo)</span></span>
      </p>
    </div></body></tt>"#;
    let parsed = lrc::parse(&ttml::to_enhanced_lrc(xml).expect("the line still converts"));
    assert_eq!(parsed.lines[0].text, "Main line");
    assert!(parsed.lines[0].secondary.is_empty());
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
    // Nothing in this one overlaps and nothing sings behind it: a second voice
    // appearing here would mean the thresholds are firing on ordinary lines.
    assert!(parsed.lines.iter().all(|l| l.secondary.is_empty()));

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
    assert_eq!(
        spotify_track_id("spotify:track:5TRPicyLGbAF2LGBFbHGvO"),
        Some(want)
    );
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
    assert!(parsed.lines.iter().all(|l| l.secondary.is_empty()));
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

#[test]
fn syllable_spans_survive_the_conversion_as_one_word() {
    // TTML splits a long word into consecutive spans with nothing between them.
    // The A2 output must keep them touching, or the display cannot tell a
    // syllable from a word.
    let xml = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div>
      <p begin="00:01.000" end="00:03.000"><span begin="00:01.000" end="00:01.200">be</span><span begin="00:01.200" end="00:01.800">lieve</span> <span begin="00:02.000" end="00:03.000">me</span></p>
    </div></body></tt>"#;
    let parsed = lrc::parse(&ttml::to_enhanced_lrc(xml).unwrap());
    let line = &parsed.lines[0];
    assert_eq!(line.text, "believe me");
    assert_eq!(line.words.len(), 3);
    assert!(line.continues_word(1), "the second span is a syllable");
    assert!(!line.continues_word(2), "the third is a separate word");
    assert_eq!(line.word_bounds(1), Some(0..7));
}
