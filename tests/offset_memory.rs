//! The sync nudge belongs to the song, not to the session.
//!
//! Before this, `,` and `.` moved one global offset: the correction that
//! rescued a badly timed file followed you into the next song, which was timed
//! fine, and broke that one instead — and it was gone by the next run anyway,
//! so the same song had to be re-tuned by ear every time it came up. Here the
//! offset is keyed by the track and written to disk, so an untuned song starts
//! from the config default and a tuned one comes back the way you left it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use terminal_lyrics::config::Config;
use terminal_lyrics::offsets::Offsets;
use terminal_lyrics::player::{PlayerEvent, Track};
use terminal_lyrics::tui::App;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "terminal-lyrics-offsets-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("offsets.json")
}

fn track(id: &str) -> Track {
    Track {
        id: id.to_string(),
        title: format!("Song {id}"),
        artist: "Some Artist".to_string(),
        album: None,
        length: Some(200.0),
    }
}

/// Load a song the way the player would: a `Track` event through the app.
fn play(app: &mut App, id: &str) {
    app.apply_player_event(
        PlayerEvent::Track(Some(Box::new(track(id)))),
        Instant::now(),
    );
}

fn app_with(path: &Path, cfg: Config) -> App {
    App::new(cfg, Offsets::new(Some(path.to_path_buf())), Instant::now())
}

#[test]
fn a_nudge_stays_with_its_own_song() {
    let path = scratch("per-song");
    let mut app = app_with(&path, Config::default());

    play(&mut app, "song-a");
    assert_eq!(app.offset_ms(), 0, "an untuned song starts at the default");
    app.nudge_offset(-300);
    assert_eq!(app.offset_ms(), -300);

    // The whole point: the next song is not dragged along with it.
    play(&mut app, "song-b");
    assert_eq!(app.offset_ms(), 0, "a different song starts fresh");

    play(&mut app, "song-a");
    assert_eq!(app.offset_ms(), -300, "and the tuned one is remembered");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_nudge_survives_a_restart() {
    let path = scratch("restart");

    let mut app = app_with(&path, Config::default());
    play(&mut app, "song-a");
    app.nudge_offset(200);
    app.nudge_offset(200);
    assert_eq!(app.offset_ms(), 400);
    drop(app);

    // A second run, reading the same file from scratch.
    let mut next = app_with(&path, Config::default());
    play(&mut next, "song-a");
    assert_eq!(
        next.offset_ms(),
        400,
        "read back from disk, not from memory"
    );

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn an_untuned_song_starts_at_the_configured_default() {
    let path = scratch("default");
    let cfg = Config {
        offset_ms: 250,
        ..Config::default()
    };

    let mut app = app_with(&path, cfg.clone());
    play(&mut app, "song-a");
    assert_eq!(app.offset_ms(), 250, "the config sets the starting point");

    app.nudge_offset(-100);
    play(&mut app, "song-b");
    assert_eq!(app.offset_ms(), 250, "still the starting point for song-b");
    play(&mut app, "song-a");
    assert_eq!(app.offset_ms(), 150);

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn reset_puts_the_song_back_to_the_default_and_forgets_it() {
    let path = scratch("reset");
    let cfg = Config {
        offset_ms: 250,
        ..Config::default()
    };

    let mut app = app_with(&path, cfg);
    play(&mut app, "song-a");
    app.nudge_offset(-1000);
    assert_eq!(app.offset_ms(), -750);

    app.reset_offset();
    assert_eq!(app.offset_ms(), 250);

    // Forgotten, not stored as 250: the file is a list of corrections, and an
    // entry pinning the default would stop tracking a later change to it.
    let saved = Offsets::new(Some(path.clone()));
    assert_eq!(saved.get("song-a"), None);
    assert!(saved.is_empty());

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn nudging_back_onto_the_default_leaves_no_entry_behind() {
    let path = scratch("no-trace");
    let mut app = app_with(&path, Config::default());

    play(&mut app, "song-a");
    app.nudge_offset(100);
    assert_eq!(Offsets::new(Some(path.clone())).len(), 1);

    app.nudge_offset(-100);
    assert_eq!(app.offset_ms(), 0);
    assert!(
        Offsets::new(Some(path.clone())).is_empty(),
        "back at the default is the same as never tuned"
    );

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_stored_offset_is_clamped_like_the_clock() {
    let path = scratch("clamp");
    let mut store = Offsets::new(Some(path.clone()));
    store.set("song-a", "Some Artist — Song a", 999_999);
    assert_eq!(store.get("song-a"), Some(30_000));

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_track_with_no_stable_id_is_simply_not_stored() {
    let path = scratch("no-id");
    let mut store = Offsets::new(Some(path.clone()));
    store.set("", "nameless", -500);
    assert!(store.is_empty(), "there is nothing to key it against");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_damaged_file_reads_as_no_corrections_rather_than_failing() {
    let path = scratch("damaged");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ not json at all").unwrap();

    let mut store = Offsets::new(Some(path.clone()));
    assert!(store.is_empty());
    // And it is repaired by the next write rather than staying broken.
    store.set("song-a", "Some Artist — Song a", -400);
    assert_eq!(Offsets::new(Some(path.clone())).get("song-a"), Some(-400));

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_store_with_nowhere_to_write_still_works_for_the_session() {
    let mut store = Offsets::in_memory();
    assert_eq!(store.path(), None);
    store.set("song-a", "Some Artist — Song a", -300);
    assert_eq!(store.get("song-a"), Some(-300));
    store.clear("song-a");
    assert!(store.is_empty());
}
