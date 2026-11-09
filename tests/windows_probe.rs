//! The SMTC parser. The backend needs a live WinRT session manager, so every
//! decision lives in `parse_sessions` and `friendly_name`.
#![cfg(target_os = "windows")]

use terminal_lyrics::player::fallback_id;
use terminal_lyrics::player::windows::{RawSession, friendly_name, parse_sessions as parse};
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;

const PLAYING: i32 = Status::Playing.0;
const PAUSED: i32 = Status::Paused.0;

/// One second, in the 100-nanosecond ticks WinRT counts a `TimeSpan` in.
const SECOND: i64 = 10_000_000;

/// An arbitrary wall-clock moment, as a WinRT `DateTime`.
const NOW: i64 = 134_000_000_000_000_000;

fn session(app_id: &str, status: i32, position_ticks: i64, title: &str) -> RawSession {
    RawSession {
        app_id: app_id.to_string(),
        status,
        position_ticks,
        updated_ticks: NOW,
        end_ticks: 233 * SECOND,
        title: title.to_string(),
        artist: "Ice Cube".to_string(),
        album: "Lethal Injection".to_string(),
    }
}

fn parse_sessions(raw: &[RawSession]) -> Vec<terminal_lyrics::player::windows::Probe> {
    parse(raw, NOW)
}

#[test]
fn a_playing_position_is_carried_forward_from_when_it_was_reported() {
    // Spotify refreshes the position only every few seconds; read raw, it
    // lags by up to that much and every drift check yanks the lyrics back.
    let mut raw = session("Spotify.exe", PLAYING, 55 * SECOND, "One");
    raw.updated_ticks = NOW - 2 * SECOND;
    let out = parse_sessions(&[raw]);
    assert!((out[0].snapshot.position - 57.0).abs() < 1e-9);
}

#[test]
fn a_paused_position_is_not_carried_forward() {
    // Spotify keeps bumping the update time while paused.
    let mut raw = session("Spotify.exe", PAUSED, 55 * SECOND, "One");
    raw.updated_ticks = NOW - 2 * SECOND;
    let out = parse_sessions(&[raw]);
    assert!((out[0].snapshot.position - 55.0).abs() < 1e-9);
}

#[test]
fn a_missing_or_future_update_time_leaves_the_position_alone() {
    for updated in [0, NOW + 3 * SECOND] {
        let mut raw = session("Spotify.exe", PLAYING, 55 * SECOND, "One");
        raw.updated_ticks = updated;
        let out = parse_sessions(&[raw]);
        assert!(
            (out[0].snapshot.position - 55.0).abs() < 1e-9,
            "updated {updated}: got {}",
            out[0].snapshot.position
        );
    }
}

#[test]
fn a_timespan_is_read_as_ticks_not_milliseconds_or_seconds() {
    // 6.55 seconds. In the wrong unit the lyrics would sit at the end of the
    // song forever, which looks like a sync bug rather than a unit bug.
    let out = parse_sessions(&[session(
        "Spotify.exe",
        PLAYING,
        65_500_000,
        "Down for Whatever",
    )]);
    assert_eq!(out.len(), 1);
    assert!(
        (out[0].snapshot.position - 6.55).abs() < 1e-9,
        "expected 6.55s, got {}",
        out[0].snapshot.position
    );
}

#[test]
fn a_track_length_comes_through_in_seconds_too() {
    let out = parse_sessions(&[session("Spotify.exe", PLAYING, 0, "Down for Whatever")]);
    let track = out[0].snapshot.track.as_ref().unwrap();
    assert_eq!(track.length, Some(233.0));
}

#[test]
fn an_unknown_end_time_is_no_length_rather_than_a_zero_length() {
    // A live stream has no end time; a zero-second length would put every
    // candidate outside the duration window and cost the song its lyrics.
    let mut raw = session("Spotify.exe", PLAYING, 0, "Some Stream");
    raw.end_ticks = 0;
    let out = parse_sessions(&[raw]);
    assert_eq!(out[0].snapshot.track.as_ref().unwrap().length, None);
}

#[test]
fn an_executable_name_becomes_the_short_name_the_other_backends_use() {
    // `spotify` on Linux has to be `spotify` here, or `--player spotify` would
    // mean different things per platform.
    assert_eq!(friendly_name("Spotify.exe"), "spotify");
    assert_eq!(friendly_name("SPOTIFY.EXE"), "spotify");
    assert_eq!(friendly_name("msedge.exe"), "msedge");
}

#[test]
fn a_packaged_apps_aumid_is_reduced_to_something_typeable() {
    assert_eq!(
        friendly_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
        "zunemusic"
    );
    // The same package with no app half after the bang.
    assert_eq!(
        friendly_name("Microsoft.ZuneMusic_8wekyb3d8bbwe"),
        "zunemusic"
    );
}

#[test]
fn an_unrecognisable_id_is_still_listed_rather_than_dropped() {
    // Better a player named oddly than a player the user cannot select at all.
    assert_eq!(friendly_name("!"), "!");
    assert_eq!(friendly_name(""), "");
}

#[test]
fn only_the_playing_status_counts_as_playing() {
    // Pinned to the raw values Spotify was seen sending: 4 playing, 5 paused.
    assert_eq!((PLAYING, PAUSED), (4, 5));
    let out = parse_sessions(&[
        session("Spotify.exe", PLAYING, 0, "One"),
        session("msedge.exe", PAUSED, 0, "Two"),
    ]);
    let spotify = out.iter().find(|p| p.name == "spotify").unwrap();
    let edge = out.iter().find(|p| p.name == "msedge").unwrap();
    assert!(spotify.snapshot.playing);
    assert!(!edge.snapshot.playing);
}

#[test]
fn a_session_with_no_title_is_a_player_without_a_track() {
    // A browser playing a video it has no metadata for is still something the
    // user might want to follow; it just has no lyrics to look up.
    let out = parse_sessions(&[session("chrome.exe", PLAYING, 0, "   ")]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "chrome");
    assert!(out[0].snapshot.track.is_none());
}

#[test]
fn the_track_id_is_the_shared_fallback_key() {
    // SMTC has no track id of its own. Using the same last-resort key as the
    // other backends is what lets a song cached on Linux hit on Windows.
    let out = parse_sessions(&[session("Spotify.exe", PLAYING, 0, "Down for Whatever")]);
    let track = out[0].snapshot.track.as_ref().unwrap();
    assert_eq!(track.id, fallback_id("Ice Cube", "Down for Whatever"));
}

#[test]
fn two_sessions_from_one_app_collapse_to_the_one_actually_playing() {
    // Two browser windows register two sessions. Without this, `--player msedge`
    // is a coin flip between the tab playing music and a paused one.
    let mut idle = session("msedge.exe", PAUSED, 0, "");
    idle.end_ticks = 0;
    let live = session("msedge.exe", PLAYING, 12 * SECOND, "Down for Whatever");

    for order in [vec![idle.clone(), live.clone()], vec![live, idle]] {
        let out = parse_sessions(&order);
        assert_eq!(out.len(), 1, "the two sessions should collapse to one");
        assert_eq!(out[0].name, "msedge");
        assert!(out[0].snapshot.playing, "the playing session should win");
        assert_eq!(
            out[0].snapshot.track.as_ref().unwrap().title,
            "Down for Whatever"
        );
    }
}

#[test]
fn surrounding_whitespace_in_metadata_is_trimmed_off() {
    let mut raw = session("Spotify.exe", PLAYING, 0, "  Down for Whatever  ");
    raw.artist = "  Ice Cube  ".to_string();
    raw.album = "   ".to_string();
    let out = parse_sessions(&[raw]);
    let track = out[0].snapshot.track.as_ref().unwrap();
    assert_eq!(track.title, "Down for Whatever");
    assert_eq!(track.artist, "Ice Cube");
    // An album of nothing but spaces is no album, not an empty one.
    assert_eq!(track.album, None);
}

#[test]
fn a_non_ascii_app_id_does_not_panic() {
    // The `.exe` trim cannot cut at a fixed byte offset: here that lands
    // mid-character and panics.
    assert_eq!(friendly_name("音楽"), "音楽");
    assert_eq!(friendly_name("音楽.exe"), "音楽");
    assert_eq!(friendly_name("Player™"), "player™");
}
