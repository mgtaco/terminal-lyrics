//! The sync nudge is keyed by track and lyrics source and saved, so a fix for
//! one badly timed song neither follows you into the next, nor onto another
//! provider's lyrics, nor is lost on exit.

use std::path::{Path, PathBuf};
use std::time::Instant;

use terminal_lyrics::config::Config;
use terminal_lyrics::lrc::Lyrics;
use terminal_lyrics::lyrics::{Found, Provider, Source};
use terminal_lyrics::offsets::Offsets;
use terminal_lyrics::player::{PlayerEvent, Track};
use terminal_lyrics::prefs::Prefs;
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
        url: None,
    }
}

/// Load a song the way the player would, a `Track` event through the app,
/// and answer it with LRCLIB's lyrics.
fn play(app: &mut App, id: &str) {
    app.apply_player_event(
        PlayerEvent::Track(Some(Box::new(track(id)))),
        Instant::now(),
    );
    answer(app, Provider::LrcLib);
}

fn answer(app: &mut App, provider: Provider) {
    app.show_found(Found {
        lyrics: Lyrics::default(),
        source: Source::Provider {
            provider,
            cached: false,
        },
        synced: true,
        raw: provider.name().to_string(),
        answer: None,
    });
}

fn app_with(path: &Path, cfg: Config) -> App {
    App::new(
        cfg,
        Offsets::new(Some(path.to_path_buf())),
        Prefs::in_memory(),
        Instant::now(),
    )
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
    assert_eq!(saved.get("song-a", "lrclib"), None);
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
    store.set("song-a", "lrclib", "Some Artist — Song a", 999_999);
    assert_eq!(store.get("song-a", "lrclib"), Some(30_000));

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_track_with_no_stable_id_is_simply_not_stored() {
    let path = scratch("no-id");
    let mut store = Offsets::new(Some(path.clone()));
    store.set("", "lrclib", "nameless", -500);
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
    store.set("song-a", "lrclib", "Some Artist — Song a", -400);
    assert_eq!(
        Offsets::new(Some(path.clone())).get("song-a", "lrclib"),
        Some(-400)
    );

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_store_with_nowhere_to_write_still_works_for_the_session() {
    let mut store = Offsets::in_memory();
    assert_eq!(store.path(), None);
    store.set("song-a", "lrclib", "Some Artist — Song a", -300);
    assert_eq!(store.get("song-a", "lrclib"), Some(-300));
    store.clear("song-a", "lrclib");
    assert!(store.is_empty());
}

#[test]
fn each_provider_keeps_its_own_offset_for_a_song() {
    let path = scratch("per-provider");
    let mut app = app_with(&path, Config::default());

    play(&mut app, "song-a");
    app.nudge_offset(-300);

    // A different provider's lyrics have their own timing error.
    answer(&mut app, Provider::KuGou);
    assert_eq!(app.offset_ms(), 0, "a new provider starts at the default");
    app.nudge_offset(800);

    answer(&mut app, Provider::LrcLib);
    assert_eq!(app.offset_ms(), -300, "back to LRCLIB's own");
    answer(&mut app, Provider::KuGou);
    assert_eq!(app.offset_ms(), 800);

    let saved = Offsets::new(Some(path.clone()));
    assert_eq!(saved.get("song-a", "lrclib"), Some(-300));
    assert_eq!(saved.get("song-a", "kugou"), Some(800));
    assert_eq!(saved.len(), 1, "still one song");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn clearing_one_provider_keeps_the_others() {
    let path = scratch("clear-one");
    let mut store = Offsets::new(Some(path.clone()));
    store.set("song-a", "lrclib", "Some Artist — Song a", -300);
    store.set("song-a", "kugou", "Some Artist — Song a", 800);

    store.clear("song-a", "kugou");
    assert_eq!(store.get("song-a", "lrclib"), Some(-300));
    store.clear("song-a", "lrclib");
    assert!(store.is_empty(), "a song with nothing left is dropped");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_nudge_with_no_lyrics_showing_is_not_saved() {
    let path = scratch("no-lyrics");
    let mut app = app_with(&path, Config::default());

    app.apply_player_event(
        PlayerEvent::Track(Some(Box::new(track("song-a")))),
        Instant::now(),
    );
    assert_eq!(app.nudge_offset(500), None, "no source to file it under");
    assert!(Offsets::new(Some(path.clone())).is_empty());

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn an_offset_file_from_before_providers_reads_as_untuned() {
    let path = scratch("version-1");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"version":1,"tracks":{"song-a":{"offset_ms":-300,"label":"x"}}}"#,
    )
    .unwrap();

    let mut app = app_with(&path, Config::default());
    play(&mut app, "song-a");
    assert_eq!(
        app.offset_ms(),
        0,
        "which provider it was tuned for is unknown"
    );

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
