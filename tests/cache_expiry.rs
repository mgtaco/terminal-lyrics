//! What the cache keeps forever, and what it lets go of.
//!
//! Two of the four providers are one person's server each. A track played
//! while both were unreachable resolves to LRCLIB's line-level answer, and
//! before this that answer was kept for good — so a five-minute outage cost
//! word timings permanently, on exactly the tracks being played during it, and
//! the only way back was knowing to press `r`. Word-timed hits are still kept
//! forever, because nothing better is coming for them.

use std::path::{Path, PathBuf};

use terminal_lyrics::lyrics::cache::Cache;

const WORD_TIMED: &str = "[00:01.000]<00:01.000>Hello<00:01.500> <00:02.000>world<00:02.500>\n";
const LINE_TIMED: &str = "[00:01.000]Hello world\n";
const KEY: &str = "https://open.spotify.com/track/70LcF31zb1H0PyJoS1Sx1r";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "terminal-lyrics-cache-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Backdate every entry in the directory. The cache reads the clock itself and
/// offers no seam for it, so the entry is aged on disk instead — which has the
/// merit of testing the format that is actually written.
fn age_by_days(dir: &Path, days: u64) {
    let mut found = false;
    for entry in std::fs::read_dir(dir).expect("cache directory") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let at = text.find("\"stored_at\":").expect("a stored_at field") + "\"stored_at\":".len();
        let end = at + text[at..].find(',').expect("a field after stored_at");
        let stored: u64 = text[at..end].parse().expect("a timestamp");
        let aged = format!("{}{}{}", &text[..at], stored - days * 24 * 60 * 60, &text[end..]);
        std::fs::write(&path, aged).unwrap();
        found = true;
    }
    assert!(found, "nothing was written to the cache to age");
}

#[test]
fn a_word_timed_hit_is_kept_indefinitely() {
    let dir = scratch("word-kept");
    let cache = Cache::new(Some(dir.clone()));
    cache.put_hit(KEY, "Radiohead - Creep", WORD_TIMED, None, true);

    age_by_days(&dir, 400);

    let got = cache.get(KEY).expect("still cached").expect("a hit");
    assert!(got.lyrics.has_word_timings());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_line_level_hit_expires_like_a_miss() {
    let dir = scratch("line-expires");
    let cache = Cache::new(Some(dir.clone()));
    cache.put_hit(KEY, "Radiohead - Creep", LINE_TIMED, Some(7), true);

    // Inside the day it is served, so a busy afternoon does not re-query the
    // same track on every restart.
    assert!(
        cache.get(KEY).expect("cached").is_some(),
        "a fresh line-level hit is still a hit"
    );

    age_by_days(&dir, 2);
    assert!(
        cache.get(KEY).is_none(),
        "a day-old line-level hit asks again, in case a better source has it now"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expiry_is_decided_by_the_lyrics_not_by_a_stored_flag() {
    // The granularity is derived from the cached document when it is read, so
    // entries written before this rule existed start expiring correctly on
    // their first read rather than needing a cache version bump to clear them.
    let dir = scratch("derived");
    let cache = Cache::new(Some(dir.clone()));

    // `synced` is true either way — it is about timestamps existing at all, not
    // about their granularity, so it cannot stand in for this decision.
    cache.put_hit(KEY, "Radiohead - Creep", LINE_TIMED, None, true);
    age_by_days(&dir, 30);
    assert!(cache.get(KEY).is_none());

    cache.put_hit(KEY, "Radiohead - Creep", WORD_TIMED, None, true);
    age_by_days(&dir, 30);
    assert!(cache.get(KEY).is_some(), "the same age, kept, because it is word-timed");
    let _ = std::fs::remove_dir_all(&dir);
}
