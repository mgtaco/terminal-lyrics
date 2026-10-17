//! The provider race and the cache around it, driven through stub providers.
//!
//! Ported from DankMaterialShell's own tests (`dankgo/lyrics`), so the two
//! engines are held to the same behaviour.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use terminal_lyrics::lyrics::cache::{self, Cache, Entry};
use terminal_lyrics::lyrics::engine::{Fetch, FetchFuture, Source};
use terminal_lyrics::lyrics::model::{Line, Lyrics, Request, Word};
use terminal_lyrics::lyrics::{Engine, Error, Origin, Provider};
use tokio::sync::Notify;

type Handler = dyn Fn(Provider, Request) -> FetchFuture + Send + Sync;

struct Stub {
    provider: Provider,
    handler: Arc<Handler>,
}

impl Fetch for Stub {
    fn fetch(&self, req: Request) -> FetchFuture {
        (self.handler)(self.provider, req)
    }
}

fn scratch() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "tl-engine-{}-{nanos}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An engine whose every provider answers through `handler`.
fn engine_at(
    dir: PathBuf,
    handler: impl Fn(Provider, Request) -> FetchFuture + Send + Sync + 'static,
) -> Arc<Engine> {
    let handler: Arc<Handler> = Arc::new(handler);
    let sources: HashMap<Provider, Source> = Provider::DEFAULT_ORDER
        .into_iter()
        .map(|provider| {
            let source = Source {
                word_sync: provider.word_sync(),
                fetch: Arc::new(Stub {
                    provider,
                    handler: handler.clone(),
                }),
            };
            (provider, source)
        })
        .collect();
    Arc::new(
        Engine::with_sources(Cache::new(Some(dir)), sources)
            .with_timing(Duration::from_millis(10), Duration::from_secs(5)),
    )
}

fn engine(
    handler: impl Fn(Provider, Request) -> FetchFuture + Send + Sync + 'static,
) -> Arc<Engine> {
    engine_at(scratch(), handler)
}

fn req() -> Request {
    Request {
        artist: "Artist".into(),
        title: "Track".into(),
        ..Default::default()
    }
}

fn plain(text: &str) -> Lyrics {
    Lyrics {
        plain: text.into(),
        ..Default::default()
    }
}

fn line_synced(text: &str) -> Lyrics {
    Lyrics {
        synced: vec![Line {
            start: 1.0,
            text: text.into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn word_synced(text: &str) -> Lyrics {
    Lyrics {
        synced: vec![Line {
            start: 1.0,
            text: text.into(),
            words: vec![Word {
                start: 1.0,
                end: 0.0,
                text: text.into(),
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn ready(lyrics: Lyrics) -> FetchFuture {
    Box::pin(async move { Ok(lyrics) })
}

fn fail(error: Error) -> FetchFuture {
    Box::pin(async move { Err(error) })
}

fn source(answer: &terminal_lyrics::lyrics::Answer) -> Option<Provider> {
    match answer.origin {
        Origin::Provider(p) => Some(p),
        _ => None,
    }
}

/// Sets its flag when dropped: how a test sees a fetch being cancelled.
struct OnDrop(Arc<AtomicBool>);

impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn priority_holds_a_fast_lower_answer_and_cancels_the_rest() {
    for high_found in [true, false] {
        let release = Arc::new(Notify::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (release2, cancelled2) = (release.clone(), cancelled.clone());
        let engine = engine(move |provider, _| {
            let release = release2.clone();
            let cancelled = cancelled2.clone();
            Box::pin(async move {
                match provider {
                    Provider::BetterLyrics => {
                        release.notified().await;
                        if high_found {
                            Ok(plain("highest priority"))
                        } else {
                            Err(Error::NotFound)
                        }
                    }
                    Provider::LrcLib => Ok(plain("second priority")),
                    _ => {
                        let _guard = OnDrop(cancelled);
                        std::future::pending::<()>().await;
                        unreachable!()
                    }
                }
            })
        });
        let order = [
            Provider::BetterLyrics,
            Provider::LrcLib,
            Provider::LyricsPlus,
        ];
        let lookup = {
            let engine = engine.clone();
            tokio::spawn(async move { engine.lookup(&req(), &order).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !lookup.is_finished(),
            "a lower priority won before the first finished"
        );
        release.notify_one();
        let answer = lookup.await.unwrap().unwrap();
        let want = if high_found {
            Provider::BetterLyrics
        } else {
            Provider::LrcLib
        };
        assert_eq!(source(&answer), Some(want));
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            cancelled.load(Ordering::SeqCst),
            "the unused provider was not cancelled"
        );
    }
}

#[tokio::test]
async fn word_sync_outranks_priority() {
    let line_done = Arc::new(Notify::new());
    let gate = line_done.clone();
    let engine = Arc::new(
        Arc::try_unwrap(engine(move |provider, _| {
            let gate = gate.clone();
            Box::pin(async move {
                if provider == Provider::LrcLib {
                    gate.notify_one();
                    return Ok(line_synced("line synced"));
                }
                gate.notified().await;
                Ok(word_synced("word synced"))
            })
        }))
        .ok()
        .unwrap()
        .with_timing(Duration::from_secs(60), Duration::from_secs(5)),
    );
    let answer = engine
        .lookup(&req(), &[Provider::LrcLib, Provider::LyricsPlus])
        .await
        .unwrap();
    assert_eq!(source(&answer), Some(Provider::LyricsPlus));
}

#[tokio::test]
async fn line_sync_outranks_plain_at_higher_priority() {
    let engine = engine(|provider, _| match provider {
        Provider::BetterLyrics => ready(plain("plain")),
        _ => ready(line_synced("line synced")),
    });
    let answer = engine
        .lookup(&req(), &[Provider::BetterLyrics, Provider::LrcLib])
        .await
        .unwrap();
    assert_eq!(source(&answer), Some(Provider::LrcLib));
}

#[tokio::test]
async fn answers_are_cached_per_provider_and_disabled_providers_are_not_asked() {
    let calls: Arc<Mutex<HashMap<Provider, usize>>> = Arc::default();
    let seen = calls.clone();
    let engine = engine(move |provider, _| {
        *seen.lock().unwrap().entry(provider).or_default() += 1;
        match provider {
            Provider::BetterLyrics => fail(Error::NotFound),
            _ => ready(plain("cached line")),
        }
    });
    let order = [Provider::BetterLyrics, Provider::LyricsPlus];
    for _ in 0..2 {
        let answer = engine.lookup(&req(), &order).await.unwrap();
        assert_eq!(source(&answer), Some(Provider::LyricsPlus));
    }
    assert_eq!(
        *calls.lock().unwrap(),
        HashMap::from([(Provider::BetterLyrics, 1), (Provider::LyricsPlus, 1)])
    );

    let answer = engine.lookup(&req(), &[]).await.unwrap();
    assert!(!answer.found);
    engine.lookup(&req(), &[Provider::LrcLib]).await.unwrap();
    assert_eq!(
        calls.lock().unwrap()[&Provider::LrcLib],
        1,
        "the cache leaked between providers"
    );
}

#[tokio::test]
async fn a_failing_provider_falls_back() {
    let engine = engine(|provider, _| match provider {
        Provider::BetterLyrics => fail(Error::Status(503)),
        _ => ready(plain("usable lyrics")),
    });
    let answer = engine
        .lookup(&req(), &[Provider::BetterLyrics, Provider::LyricsPlus])
        .await
        .unwrap();
    assert!(answer.found);
    assert_eq!(answer.lyrics.plain, "usable lyrics");
}

#[tokio::test]
async fn errors_are_not_cached() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let engine = engine(move |_, _| {
        seen.fetch_add(1, Ordering::SeqCst);
        fail(Error::Status(503))
    });
    for _ in 0..2 {
        assert!(
            engine
                .lookup(&req(), &[Provider::BetterLyrics])
                .await
                .is_err()
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_definite_miss_outranks_a_failure() {
    let engine = engine(|provider, _| match provider {
        Provider::BetterLyrics => fail(Error::Status(429)),
        _ => ready(Lyrics::default()),
    });
    let answer = engine
        .lookup(&req(), &[Provider::BetterLyrics, Provider::LrcLib])
        .await
        .unwrap();
    assert!(!answer.found);
}

#[tokio::test]
async fn every_provider_failing_stays_an_error() {
    let engine = engine(|_, _| fail(Error::Status(429)));
    assert!(
        engine
            .lookup(&req(), &[Provider::BetterLyrics, Provider::LrcLib])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn cache_only_never_fetches() {
    let engine = engine(|_, _| panic!("the lookup reached the network"));
    let request = Request {
        cache_only: true,
        ..req()
    };
    let answer = engine.lookup(&request, &[Provider::LrcLib]).await.unwrap();
    assert!(!answer.found);
}

#[tokio::test]
async fn a_file_next_to_the_track_beats_the_network() {
    let engine = engine(|_, _| panic!("the lookup reached the network"));
    let dir = scratch();
    std::fs::write(dir.join("track.lrc"), "[00:11.82]What a thrill").unwrap();
    let url = reqwest::Url::from_file_path(dir.join("track.flac")).unwrap();
    let request = Request {
        file_url: url.to_string(),
        ..Default::default()
    };
    let answer = engine
        .lookup(&request, &Provider::DEFAULT_ORDER)
        .await
        .unwrap();
    assert!(matches!(answer.origin, Origin::Sidecar(_)));
    assert_eq!(answer.lyrics.synced.len(), 1);
}

#[tokio::test]
async fn a_miss_is_cached_and_not_asked_again() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let engine = engine(move |_, _| {
        seen.fetch_add(1, Ordering::SeqCst);
        fail(Error::NotFound)
    });
    for _ in 0..2 {
        engine.lookup(&req(), &[Provider::LrcLib]).await.unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn not_cached_by_the_provider_is_not_stored_as_a_miss() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let engine = engine(move |_, _| {
        seen.fetch_add(1, Ordering::SeqCst);
        fail(Error::Uncached)
    });
    for _ in 0..2 {
        let answer = engine
            .lookup(&req(), &[Provider::BetterLyrics])
            .await
            .unwrap();
        assert!(!answer.found);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

fn write_entry(dir: &std::path::Path, key: &str, entry: &Entry) {
    let path = dir.join(&key[..2]).join(format!("{key}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(entry).unwrap()).unwrap();
}

fn days_ago(days: i64) -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - days * 24 * 60 * 60
}

#[test]
fn a_miss_expires_after_two_weeks() {
    let dir = scratch();
    let cache = Cache::new(Some(dir.clone()));
    let key = cache::track_key(&req());
    let mut entry = Entry {
        version: cache::CACHE_VERSION,
        miss: true,
        fetched: days_ago(15),
        ..Default::default()
    };
    write_entry(&dir, &key, &entry);
    assert!(cache.load(&key).is_none(), "an expired miss was served");
    entry.fetched = days_ago(0);
    write_entry(&dir, &key, &entry);
    assert!(cache.load(&key).is_some(), "a fresh miss was not served");
}

#[tokio::test]
async fn a_stale_line_synced_entry_upgrades_to_word_sync() {
    for upgraded in [true, false] {
        let dir = scratch();
        let key = cache::provider_key(Provider::LrcLib, &req());
        write_entry(
            &dir,
            &key,
            &Entry {
                version: cache::CACHE_VERSION,
                fetched: days_ago(15),
                source: "lrclib".into(),
                lyrics: line_synced("cached"),
                ..Default::default()
            },
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let engine = engine_at(dir, move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
            ready(if upgraded {
                word_synced("refetched")
            } else {
                line_synced("refetched")
            })
        });
        let mut answer = None;
        for _ in 0..2 {
            answer = Some(engine.lookup(&req(), &[Provider::LrcLib]).await.unwrap());
        }
        let want = if upgraded { "refetched" } else { "cached" };
        assert_eq!(calls.load(Ordering::SeqCst), 1, "upgraded={upgraded}");
        assert_eq!(
            answer.unwrap().lyrics.synced[0].text,
            want,
            "upgraded={upgraded}"
        );
    }
}

#[test]
fn the_cache_key_folds_identity_but_not_duration() {
    let base = Request {
        artist: "Daft Punk".into(),
        title: "Get Lucky".into(),
        album: "Random Access Memories".into(),
        duration: 369.0,
        ..Default::default()
    };
    let folded = Request {
        artist: "  daft   PUNK ".into(),
        title: "get lucky".into(),
        album: "random access memories".into(),
        ..base.clone()
    };
    assert_eq!(cache::track_key(&base), cache::track_key(&folded));
    let other_cut = Request {
        duration: 248.0,
        ..base.clone()
    };
    assert_ne!(cache::track_key(&base), cache::track_key(&other_cut));
}

#[tokio::test]
async fn the_cached_view_takes_the_best_entry_without_asking() {
    let dir = scratch();
    let r = req();
    let mut entry = Entry {
        source: "lrclib".into(),
        lyrics: line_synced("line"),
        ..Default::default()
    };
    Cache::new(Some(dir.clone())).store(&cache::provider_key(Provider::LrcLib, &r), &mut entry);
    let mut entry = Entry {
        source: "kugou".into(),
        lyrics: word_synced("word"),
        ..Default::default()
    };
    Cache::new(Some(dir.clone())).store(&cache::provider_key(Provider::KuGou, &r), &mut entry);
    let engine = engine_at(dir, |_, _| panic!("the cached view reached the network"));
    let answer = engine.cached(&r, &Provider::DEFAULT_ORDER).unwrap();
    assert_eq!(source(&answer), Some(Provider::KuGou));
    assert!(answer.cached);
}
