//! Candidate scoring, query relaxation, and the cache round-trip.

use terminal_lyrics::lyrics::cache::Cache;
use terminal_lyrics::lyrics::lrclib::{Record, pick_best};
use terminal_lyrics::lyrics::normalize;

fn rec(id: i64, title: &str, artist: &str, duration: f64, synced: bool) -> Record {
    Record {
        id,
        track_name: title.into(),
        artist_name: artist.into(),
        album_name: None,
        duration: Some(duration),
        instrumental: false,
        plain_lyrics: Some("words".into()),
        synced_lyrics: synced.then(|| "[00:10.00]words".to_string()),
    }
}

#[test]
fn duration_decides_between_same_titled_records() {
    // Modelled on the real response for "Kanye West – Flashing Lights": the
    // first row is an 85s-longer edit, so taking row zero gets the wrong song.
    let results = vec![
        rec(1, "Flashing Lights", "Kanye West", 322.0, true),
        rec(2, "Flashing Lights", "Kanye West", 244.0, true),
        rec(3, "Flashing Lights", "Kanye West", 237.0, true),
        rec(4, "Flashing Lights", "Kanye West", 198.0, true),
    ];
    let best = pick_best(results, Some(237.5), "Flashing Lights", "Kanye West").unwrap();
    assert_eq!(best.id, 3);
}

#[test]
fn records_far_from_the_known_duration_are_rejected_entirely() {
    let results = vec![rec(1, "Song", "Artist", 400.0, true)];
    // Better to show nothing than to run a whole song out of sync.
    assert!(pick_best(results, Some(200.0), "Song", "Artist").is_none());
}

#[test]
fn synced_beats_unsynced_even_at_a_slightly_worse_duration() {
    let results = vec![
        rec(1, "Song", "Artist", 200.0, false),
        rec(2, "Song", "Artist", 203.0, true),
    ];
    let best = pick_best(results, Some(200.0), "Song", "Artist").unwrap();
    assert_eq!(best.id, 2);
}

#[test]
fn without_a_known_duration_the_best_titled_synced_record_wins() {
    let results = vec![
        rec(1, "Song (Live)", "Artist", 300.0, true),
        rec(2, "Song", "Artist", 200.0, true),
    ];
    let best = pick_best(results, None, "Song", "Artist").unwrap();
    assert_eq!(best.id, 2);
}

#[test]
fn records_with_no_usable_text_are_never_chosen() {
    let mut empty = rec(1, "Song", "Artist", 200.0, false);
    empty.plain_lyrics = None;
    empty.synced_lyrics = None;
    let results = vec![empty, rec(2, "Song", "Artist", 201.0, true)];
    assert_eq!(pick_best(results, Some(200.0), "Song", "Artist").unwrap().id, 2);
}

#[test]
fn an_empty_result_set_is_a_miss_not_a_panic() {
    assert!(pick_best(vec![], Some(200.0), "Song", "Artist").is_none());
}

#[test]
fn relaxing_a_title_strips_only_decorations() {
    assert_eq!(
        normalize::relax_title("Bohemian Rhapsody - Remastered 2011").as_deref(),
        Some("Bohemian Rhapsody")
    );
    assert_eq!(
        normalize::relax_title("Sicko Mode (feat. Drake)").as_deref(),
        Some("Sicko Mode")
    );
    assert_eq!(
        normalize::relax_title("Song - Live - Remastered").as_deref(),
        Some("Song")
    );
    // Parentheses that are part of the title survive.
    assert_eq!(normalize::relax_title("(Don't Fear) The Reaper"), None);
    // A dash that is part of the title survives.
    assert_eq!(normalize::relax_title("Marie - Anne"), None);
    // Nothing to strip means no second request.
    assert_eq!(normalize::relax_title("Creep"), None);
}

#[test]
fn primary_artist_takes_the_first_credit_only() {
    assert_eq!(
        normalize::primary_artist("Travis Scott feat. Drake").as_deref(),
        Some("Travis Scott")
    );
    assert_eq!(
        normalize::primary_artist("Calvin Harris & Dua Lipa").as_deref(),
        Some("Calvin Harris")
    );
    assert_eq!(normalize::primary_artist("Radiohead"), None);
}

#[test]
fn the_cache_round_trips_timestamps_not_plain_text() {
    // Regression guard: storing `plain_text()` would return unsynced lyrics on
    // every hit, so the first play would be synced and every later one not.
    let dir = std::env::temp_dir().join(format!("lyrics-cache-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = Cache::new(Some(dir.clone()));

    let raw = "[00:10.00]first line\n[00:20.00]second line\n";
    cache.put_hit("track-key", "Artist — Title", raw, Some(42), true);

    let hit = cache.get("track-key").expect("should be cached").expect("should be a hit");
    assert!(hit.synced);
    assert_eq!(hit.lyrics.lines.len(), 2);
    assert_eq!(hit.lyrics.lines[0].start, 10.0);
    assert_eq!(hit.lyrics.lines[1].start, 20.0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_remembered_miss_is_distinct_from_never_having_looked() {
    let dir = std::env::temp_dir().join(format!("lyrics-miss-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = Cache::new(Some(dir.clone()));

    assert!(cache.get("unknown").is_none(), "nothing cached yet");
    cache.put_miss("unknown", "Artist — Title");
    assert!(
        matches!(cache.get("unknown"), Some(None)),
        "a miss must be remembered so it is not re-queried every track change"
    );

    cache.forget("unknown");
    assert!(cache.get("unknown").is_none(), "refetch must clear it");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cache_with_nowhere_to_write_degrades_quietly() {
    let cache = Cache::new(None);
    cache.put_hit("k", "l", "[00:01.00]x", None, true);
    assert!(cache.get("k").is_none());
    assert!(cache.dir().is_none());
}

#[test]
fn entries_written_by_an_older_version_are_not_served() {
    // Cached lyrics outlive the code that wrote them. When a parser bug is
    // fixed, a stale hit would keep serving the broken result forever, so the
    // version stamp must invalidate it.
    let dir = std::env::temp_dir().join(format!("lyrics-version-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let cache = Cache::new(Some(dir.clone()));
    cache.put_hit("k", "label", "[00:10.00]words", None, true);

    // Rewrite the stored entry as if an older build had produced it.
    let file = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "json"))
        .expect("entry should exist");
    // Stamp it version 0 without naming the current one: this test has to keep
    // working across every future bump, not just the one it was written for.
    let text = std::fs::read_to_string(&file).unwrap();
    let at = text.find("\"version\":").expect("entries carry a version") + "\"version\":".len();
    let end = at + text[at..].find(|c: char| !c.is_ascii_digit()).unwrap();
    std::fs::write(&file, format!("{}0{}", &text[..at], &text[end..])).unwrap();

    assert!(
        cache.get("k").is_none(),
        "an entry from an older version must be refetched, not served"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
