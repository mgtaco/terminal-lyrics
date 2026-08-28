//! Parsing the AppleScript probe.
//!
//! The backend itself cannot be tested — it shells out to `osascript`, which
//! would prompt for Automation access locally and fail outright in CI — so the
//! edge is kept as thin as possible and everything with a decision in it lives
//! in `parse_probe`. These are the decisions.
#![cfg(target_os = "macos")]

use terminal_lyrics::lyrics::amll::spotify_track_id;
use terminal_lyrics::player::macos::{ProbeError, parse_probe};

/// name, state, position ms, duration ms, id, title, artist, album.
fn row(fields: &[&str]) -> String {
    fields.join("\t")
}

#[test]
fn a_spotify_row_becomes_a_track_with_seconds_not_milliseconds() {
    let out = parse_probe(&row(&[
        "spotify",
        "playing",
        "6550",
        "233000",
        "spotify:track:2Ih217RCGAmyQR68Nn7Cqo",
        "You Know How We Do It",
        "Ice Cube",
        "Lethal Injection",
    ]));
    assert_eq!(out.len(), 1);
    let snap = out[0].result.as_ref().unwrap();
    assert!(snap.playing);
    assert!((snap.position - 6.55).abs() < 1e-9);

    let track = snap.track.as_ref().unwrap();
    assert_eq!(track.title, "You Know How We Do It");
    assert_eq!(track.artist, "Ice Cube");
    assert_eq!(track.album.as_deref(), Some("Lethal Injection"));
    // 233 seconds, not 233000. A wrong unit here is not a visible failure: it
    // just puts every LRCLIB candidate outside the five-second window and the
    // track silently gets no lyrics.
    assert!((track.length.unwrap() - 233.0).abs() < 1e-9);
}

#[test]
fn the_spotify_id_survives_intact_and_is_a_valid_amll_key() {
    let out = parse_probe(&row(&[
        "spotify",
        "playing",
        "0",
        "233000",
        "spotify:track:2Ih217RCGAmyQR68Nn7Cqo",
        "t",
        "a",
        "",
    ]));
    let track = out[0].result.as_ref().unwrap().track.as_ref().unwrap();
    assert_eq!(track.id, "spotify:track:2Ih217RCGAmyQR68Nn7Cqo");
    // This is the whole reason the macOS backend drives Spotify by AppleScript:
    // the id it hands back is the word-timed database's lookup key.
    assert_eq!(spotify_track_id(&track.id), Some("2Ih217RCGAmyQR68Nn7Cqo"));
}

#[test]
fn a_music_row_keeps_its_persistent_id_as_the_cache_key() {
    let out = parse_probe(&row(&[
        "music",
        "playing",
        "1000",
        "180000",
        "A1B2C3D4E5F60718",
        "Title",
        "Artist",
        "Album",
    ]));
    let track = out[0].result.as_ref().unwrap().track.as_ref().unwrap();
    assert_eq!(track.id, "A1B2C3D4E5F60718");
    // Nothing to look up in AMLL: that path is Spotify-only.
    assert_eq!(spotify_track_id(&track.id), None);
}

#[test]
fn a_paused_player_is_not_reported_as_playing() {
    let out = parse_probe(&row(&[
        "spotify", "paused", "6550", "233000", "id", "t", "a", "al",
    ]));
    assert!(!out[0].result.as_ref().unwrap().playing);
}

#[test]
fn an_app_running_with_nothing_loaded_reports_no_track() {
    // What a stopped player produces: the state came back, the `current track`
    // access did not.
    let out = parse_probe(&row(&["music", "stopped", "0", "0", "", "", "", ""]));
    assert_eq!(out.len(), 1);
    let snap = out[0].result.as_ref().unwrap();
    assert!(!snap.playing);
    assert!(snap.track.is_none());
}

#[test]
fn an_absent_album_is_none_rather_than_an_empty_string() {
    let out = parse_probe(&row(&[
        "spotify", "playing", "0", "233000", "id", "Title", "Artist", "",
    ]));
    let track = out[0].result.as_ref().unwrap().track.as_ref().unwrap();
    assert_eq!(track.album, None);
}

#[test]
fn a_duration_of_zero_is_absent_rather_than_a_zero_length_track() {
    let out = parse_probe(&row(&[
        "spotify", "playing", "0", "0", "id", "Title", "Artist", "Album",
    ]));
    let track = out[0].result.as_ref().unwrap().track.as_ref().unwrap();
    // `Some(0.0)` would be offered to LRCLIB as a real length and reject every
    // candidate; `None` lets the match fall back to artist and title.
    assert_eq!(track.length, None);
}

#[test]
fn a_track_with_no_id_falls_back_to_artist_and_title() {
    let out = parse_probe(&row(&[
        "music",
        "playing",
        "0",
        "180000",
        "",
        "Creep",
        "Radiohead",
        "Pablo Honey",
    ]));
    let track = out[0].result.as_ref().unwrap().track.as_ref().unwrap();
    assert_eq!(
        track.id,
        terminal_lyrics::player::fallback_id("Radiohead", "Creep")
    );
}

#[test]
fn titles_keep_the_punctuation_and_non_ascii_they_arrive_with() {
    let out = parse_probe(&row(&[
        "spotify",
        "playing",
        "0",
        "1000",
        "id",
        "サマータイムレコード (feat. someone) — 2011 Remaster",
        "じん",
        "メカクシティレコーズ",
    ]));
    let track = out[0].result.as_ref().unwrap().track.as_ref().unwrap();
    assert_eq!(
        track.title,
        "サマータイムレコード (feat. someone) — 2011 Remaster"
    );
    assert_eq!(track.artist, "じん");
}

#[test]
fn a_tab_inside_a_title_cannot_corrupt_the_position_or_the_length() {
    // The free-text fields come last for exactly this reason. A tab in a title
    // can only spill into artist and album, never back into the numbers.
    let out = parse_probe(&row(&[
        "spotify",
        "playing",
        "6550",
        "233000",
        "spotify:track:2Ih217RCGAmyQR68Nn7Cqo",
        "Odd\tTitle",
        "Artist",
        "Album",
    ]));
    let snap = out[0].result.as_ref().unwrap();
    assert!((snap.position - 6.55).abs() < 1e-9);
    let track = snap.track.as_ref().unwrap();
    assert!((track.length.unwrap() - 233.0).abs() < 1e-9);
    assert_eq!(track.title, "Odd");
}

#[test]
fn a_truncated_row_is_skipped_rather_than_half_believed() {
    assert!(parse_probe("spotify\tplaying\t6550").is_empty());
    assert!(parse_probe("").is_empty());
    assert!(parse_probe("\n\n").is_empty());
}

#[test]
fn a_row_for_an_app_we_do_not_know_is_ignored() {
    // Nothing should be able to inject a player name we never asked about.
    let out = parse_probe(&row(&[
        "vlc", "playing", "0", "1000", "id", "Title", "Artist", "Album",
    ]));
    assert!(out.is_empty());
}

#[test]
fn a_refused_automation_prompt_is_told_apart_from_a_missing_app() {
    let denied = parse_probe(&row(&["spotify", "!error", "-1743", "0", "", "", "", ""]));
    assert_eq!(denied[0].result, Err(ProbeError::NotAuthorised));

    // -1728 is what an app that is not installed produces. Reporting that as a
    // permissions problem would send the user to Settings for nothing.
    let missing = parse_probe(&row(&["music", "!error", "-1728", "0", "", "", "", ""]));
    assert_eq!(missing[0].result, Err(ProbeError::Unavailable));
}

#[test]
fn both_players_are_read_from_one_probe() {
    let text = format!(
        "{}\n{}",
        row(&["spotify", "playing", "1000", "233000", "sid", "A", "B", "C"]),
        row(&["music", "paused", "2000", "180000", "mid", "D", "E", "F"]),
    );
    let out = parse_probe(&text);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].name, "spotify");
    assert_eq!(out[1].name, "music");
    assert!(out[0].result.as_ref().unwrap().playing);
    assert!(!out[1].result.as_ref().unwrap().playing);
}
