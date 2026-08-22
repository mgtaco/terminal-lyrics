//! The sync engine driven by a scripted player — no bus, no sleeping.

use std::time::{Duration, Instant};

use terminal_lyrics::player::fake::{FakePlayer, at};
use terminal_lyrics::player::{PlayerEvent, Track};
use terminal_lyrics::sync::{Change, SyncEngine};

fn track(id: &str) -> Box<Track> {
    Box::new(Track {
        id: id.into(),
        title: "Title".into(),
        artist: "Artist".into(),
        album: None,
        length: Some(200.0),
    })
}

fn engine(now: Instant) -> SyncEngine {
    SyncEngine::new(0, Duration::from_millis(250), now)
}

#[test]
fn position_advances_between_events_without_polling() {
    let t0 = Instant::now();
    let mut e = engine(t0);
    e.apply(
        PlayerEvent::Status {
            playing: true,
            position: 10.0,
        },
        t0,
    );
    let later = t0 + Duration::from_secs(5);
    assert!((e.lyric_position(later) - 15.0).abs() < 0.01);
}

#[test]
fn pause_freezes_the_position_and_resume_continues_it() {
    let t0 = Instant::now();
    let mut e = engine(t0);
    e.apply(
        PlayerEvent::Status {
            playing: true,
            position: 10.0,
        },
        t0,
    );

    let paused_at = t0 + Duration::from_secs(2);
    e.apply(
        PlayerEvent::Status {
            playing: false,
            position: 12.0,
        },
        paused_at,
    );

    // Ten seconds of wall clock pass while paused; the lyrics must not move.
    let much_later = paused_at + Duration::from_secs(10);
    assert!((e.lyric_position(much_later) - 12.0).abs() < 0.01);
    assert!(!e.is_playing());

    e.apply(
        PlayerEvent::Status {
            playing: true,
            position: 12.0,
        },
        much_later,
    );
    let after = much_later + Duration::from_secs(3);
    assert!((e.lyric_position(after) - 15.0).abs() < 0.01);
}

#[test]
fn a_seek_re_anchors_immediately() {
    let t0 = Instant::now();
    let mut e = engine(t0);
    e.apply(
        PlayerEvent::Status {
            playing: true,
            position: 10.0,
        },
        t0,
    );
    let seek_at = t0 + Duration::from_secs(1);
    assert_eq!(
        e.apply(PlayerEvent::Seeked { position: 90.0 }, seek_at),
        Change::Resynced
    );
    assert!((e.lyric_position(seek_at) - 90.0).abs() < 0.01);
    assert!(e.is_playing(), "a seek must not stop playback");
}

#[test]
fn a_silent_seek_is_caught_by_the_position_tick() {
    // This is the Spotify path: it does not emit `Seeked`, so the only signal
    // that the user scrubbed is the next background `Position` read.
    let t0 = Instant::now();
    let mut e = engine(t0);
    e.apply(
        PlayerEvent::Status {
            playing: true,
            position: 10.0,
        },
        t0,
    );

    let tick_at = t0 + Duration::from_secs(1);
    let change = e.apply(
        PlayerEvent::Tick {
            position: 120.0,
            playing: true,
        },
        tick_at,
    );
    assert_eq!(change, Change::Resynced);
    assert!((e.lyric_position(tick_at) - 120.0).abs() < 0.01);
}

#[test]
fn ordinary_ticks_do_not_jitter_the_display() {
    let t0 = Instant::now();
    let mut e = engine(t0);
    e.apply(
        PlayerEvent::Status {
            playing: true,
            position: 10.0,
        },
        t0,
    );
    // A tick that agrees with the prediction to within the threshold must not
    // re-anchor, or the sweep would visibly stutter once a second.
    let tick_at = t0 + Duration::from_secs(1);
    let change = e.apply(
        PlayerEvent::Tick {
            position: 11.05,
            playing: true,
        },
        tick_at,
    );
    assert_eq!(change, Change::None);
    assert!((e.lyric_position(tick_at) - 11.0).abs() < 0.01);
}

#[test]
fn repeated_metadata_for_the_same_track_does_not_trigger_a_refetch() {
    // Spotify re-emits metadata when album art arrives. Treating that as a new
    // track would clear the lyrics and hit the network again.
    let t0 = Instant::now();
    let mut e = engine(t0);
    assert!(matches!(
        e.apply(PlayerEvent::Track(Some(track("id-1"))), t0),
        Change::Track(_)
    ));
    assert_eq!(
        e.apply(PlayerEvent::Track(Some(track("id-1"))), t0),
        Change::None
    );
    assert!(matches!(
        e.apply(PlayerEvent::Track(Some(track("id-2"))), t0),
        Change::Track(_)
    ));
}

#[test]
fn a_scripted_session_ends_where_the_arithmetic_says_it_should() {
    let t0 = Instant::now();
    let mut player = FakePlayer::new(
        t0,
        vec![
            at(0, PlayerEvent::Track(Some(track("song")))),
            at(
                0,
                PlayerEvent::Status {
                    playing: true,
                    position: 0.0,
                },
            ),
            // Silent scrub to 1:00, only visible via a tick.
            at(
                3_000,
                PlayerEvent::Tick {
                    position: 60.0,
                    playing: true,
                },
            ),
            // Pause two seconds later.
            at(
                5_000,
                PlayerEvent::Status {
                    playing: false,
                    position: 62.0,
                },
            ),
            // Resume after a long gap.
            at(
                20_000,
                PlayerEvent::Status {
                    playing: true,
                    position: 62.0,
                },
            ),
        ],
    );

    let mut e = engine(t0);
    let mut track_changes = 0;
    for step_ms in [0u64, 3_000, 5_000, 20_000] {
        let now = t0 + Duration::from_millis(step_ms);
        for ev in player.drain_until(now) {
            if let Change::Track(_) = e.apply(ev, now) {
                track_changes += 1;
            }
        }
    }
    assert!(player.is_finished());
    assert_eq!(track_changes, 1);

    // 62s at resume, plus 4s of playback afterwards.
    let end = t0 + Duration::from_millis(24_000);
    assert!((e.lyric_position(end) - 66.0).abs() < 0.01, "got {}", e.lyric_position(end));
}

#[test]
fn the_offset_nudge_shifts_lookup_without_touching_playback() {
    let t0 = Instant::now();
    let mut e = engine(t0);
    e.apply(
        PlayerEvent::Status {
            playing: true,
            position: 30.0,
        },
        t0,
    );
    e.clock_mut().set_offset_ms(500);
    // Positive offset shows lyrics later, i.e. looks up an earlier position.
    assert!((e.lyric_position(t0) - 29.5).abs() < 0.01);
    e.clock_mut().nudge_offset_ms(-1000);
    assert!((e.lyric_position(t0) - 30.5).abs() < 0.01);
    // And it is bounded, so a stuck key cannot send it to another song.
    e.clock_mut().set_offset_ms(i64::MAX);
    assert_eq!(e.clock().offset_ms(), 30_000);
}
