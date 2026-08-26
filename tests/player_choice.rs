//! Choosing between players when the user has not named one.
//!
//! This exists because of a real failure: a browser registers an idle
//! `org.mpris.MediaPlayer2.chromium.instance26065` that sorts before `spotify`
//! and reports no track, so taking the first name alphabetically meant
//! following silence while music played next to it.

use terminal_lyrics::player::{PlayerState, rank_players};

fn player(name: &str, playing: bool, has_track: bool) -> PlayerState {
    PlayerState {
        name: name.into(),
        playing,
        has_track,
    }
}

#[test]
fn a_playing_player_beats_an_idle_one_that_sorts_first() {
    let chosen = rank_players(vec![
        player("chromium.instance26065", false, false),
        player("spotify", true, true),
    ])
    .unwrap();
    assert_eq!(chosen.name, "spotify");
}

#[test]
fn a_paused_player_with_a_track_beats_one_with_nothing_loaded() {
    let chosen = rank_players(vec![
        player("chromium.instance1", false, false),
        player("vlc", false, true),
    ])
    .unwrap();
    assert_eq!(chosen.name, "vlc");
}

#[test]
fn playing_beats_merely_paused() {
    let chosen = rank_players(vec![
        player("aaa_paused", false, true),
        player("zzz_playing", true, true),
    ])
    .unwrap();
    assert_eq!(chosen.name, "zzz_playing");
}

#[test]
fn equal_candidates_fall_back_to_a_stable_alphabetical_choice() {
    let names = ["mpv", "spotify", "vlc"];
    let first = rank_players(names.iter().map(|n| player(n, true, true)).collect()).unwrap();
    // Reversed input must give the same answer, or the followed player would
    // change between runs for no reason.
    let second = rank_players(names.iter().rev().map(|n| player(n, true, true)).collect()).unwrap();
    assert_eq!(first.name, "mpv");
    assert_eq!(first, second);
}

#[test]
fn something_is_chosen_even_when_every_player_is_idle() {
    let chosen = rank_players(vec![
        player("chromium.instance2", false, false),
        player("chromium.instance1", false, false),
    ])
    .unwrap();
    assert_eq!(chosen.name, "chromium.instance1");
}

#[test]
fn no_players_means_no_choice() {
    assert!(rank_players(vec![]).is_none());
}
