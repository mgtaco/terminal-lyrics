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
    assert!(
        found
            .raw
            .starts_with("[00:19.980]<00:19.980>When<00:20.320> ")
    );
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
    assert!(
        found.synced,
        "line level is still synced, just not per word"
    );
    assert!(!found.lyrics.has_word_timings());
    assert!(
        !found.raw.contains('<'),
        "line-level output must not carry word tags: {:?}",
        found.raw
    );
    assert_eq!(
        found.raw,
        "[00:12.500]First line, no word tags\n[00:15.000]Second line\n"
    );
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
    assert!(
        lrcmux::from_response(&resp, Some(238.6)).is_some(),
        "the same edit"
    );
    assert!(
        lrcmux::from_response(&resp, Some(242.0)).is_some(),
        "within 5s"
    );
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

/// The upstream filter, which is what keeps KuGou's confidently-timed wrong
/// words off the screen. Parsing is the whole of it: the value goes straight
/// out as lrcmux's own `sources` parameter.
mod sources {
    use terminal_lyrics::lyrics::lrcmux::Sources;

    fn parse(s: &str) -> Sources {
        s.parse().expect("should parse")
    }

    #[test]
    fn the_default_is_a_deny_list_and_survives_a_round_trip() {
        let d: Sources = Sources::DEFAULT.parse().expect("the default must parse");
        assert_eq!(d.param().as_deref(), Some("!kugou"));
        assert_eq!(d.to_string().parse::<Sources>().unwrap(), d);
    }

    #[test]
    fn an_empty_filter_sends_no_parameter_at_all() {
        // Not `sources=`, which would be a filter allowing nothing.
        assert_eq!(Sources::any().param(), None);
        assert_eq!(parse("").param(), None);
        assert_eq!(parse("  ,  ,").param(), None);
    }

    #[test]
    fn names_are_normalised_but_not_checked_against_a_list() {
        // The set of upstreams belongs to the server — it has both grown and
        // shrunk — so an unrecognised name is passed through rather than
        // rejected. `lyrics status` names whoever actually answered.
        assert_eq!(
            parse(" MusixMatch , ytmusic ").param().as_deref(),
            Some("musixmatch,ytmusic")
        );
        assert_eq!(
            parse("somethingnew").param().as_deref(),
            Some("somethingnew")
        );
    }

    #[test]
    fn allow_and_deny_forms_cannot_be_mixed() {
        // An allow-list already excludes everything it does not name, so a mix
        // means the user expects one half to do something it cannot.
        assert!("musixmatch,!kugou".parse::<Sources>().is_err());
        assert!("!kugou,musixmatch".parse::<Sources>().is_err());
        assert!(parse("!kugou,!genius").param().is_some());
        assert!(parse("musixmatch,ytmusic").param().is_some());
    }

    #[test]
    fn a_name_that_would_not_survive_the_wire_is_rejected() {
        // A bare `!`, or a name with a separator inside it, would arrive at the
        // server as something the user never wrote.
        assert!("!".parse::<Sources>().is_err());
        assert!("mux match".parse::<Sources>().is_err());
    }
}

/// How the request is spelled, which is load-bearing in a way that is invisible
/// from the outside: a filter the server misreads restricts it to *nothing* and
/// answers 404, which looks exactly like lrcmux being down.
mod request {
    use terminal_lyrics::lyrics::lrcmux::{self, Sources};
    use terminal_lyrics::player::Track;

    fn track() -> Track {
        Track {
            id: String::new(),
            title: "Hot N Cold".to_string(),
            artist: "Katy Perry".to_string(),
            album: None,
            length: None,
        }
    }

    fn query(sources: &str) -> String {
        let s: Sources = if sources.is_empty() {
            Sources::any()
        } else {
            sources.parse().expect("should parse")
        };
        lrcmux::request_url("https://api.lrcmux.dev", &s, &track())
            .expect("should build")
            .query()
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn the_exclusion_marker_is_sent_literally_not_percent_encoded() {
        // The bug this test exists for. lrcmux does not percent-decode
        // `sources`, so `%21kugou` is an upstream by that name, nothing
        // matches, and every lookup comes back 404.
        let q = query("!kugou");
        assert!(q.ends_with("sources=!kugou"), "got {q}");
        assert!(!q.contains("%21"), "the `!` must survive the wire: {q}");
    }

    #[test]
    fn an_empty_filter_adds_no_parameter() {
        let q = query("");
        assert!(!q.contains("sources"), "got {q}");
    }

    #[test]
    fn everything_else_is_still_escaped_normally() {
        // Only `sources` skips the encoder; a slash in an artist name must not
        // ride along unescaped into the path.
        let mut t = track();
        t.artist = "AC/DC".to_string();
        t.title = "T.N.T.".to_string();
        let url = lrcmux::request_url("https://api.lrcmux.dev/", &Sources::any(), &t).unwrap();
        assert_eq!(url.path(), "/get", "the base's trailing slash is trimmed");
        assert!(url.query().unwrap().contains("artist=AC%2FDC"), "got {url}");
    }
}
