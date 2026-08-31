//! The provider chain: who is asked, in what order, and what happens when one
//! of them is down.
//!
//! The providers sit behind the `Providers` trait precisely so this can be
//! asserted without a network. What is being protected here is not a
//! calculation but a sequence — and a wrong sequence shows up only as lyrics
//! that are quietly worse than the ones that were available.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use terminal_lyrics::lrc;
use terminal_lyrics::lyrics::cache::Cache;
use terminal_lyrics::lyrics::{Found, Outcome, Provider, Providers, Source};
use terminal_lyrics::player::Track;

#[derive(Clone, Copy, PartialEq)]
enum Answer {
    /// Has the lyrics, word-timed.
    Hit,
    /// Has the lyrics, but only timed a line at a time — which is a real
    /// answer, and still the wrong one to stop on if a better source is left
    /// to ask.
    LineHit,
    /// Has lyrics, word-timed, and timed against a longer recording: the last
    /// line starts after this track has already ended.
    PastEnd,
    /// Working, and does not have them.
    Miss,
    /// Down.
    Boom,
}

struct Fake {
    answers: Vec<(Provider, Answer)>,
    calls: Mutex<Vec<Provider>>,
}

impl Fake {
    fn new(answers: &[(Provider, Answer)]) -> Self {
        Self {
            answers: answers.to_vec(),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Everything misses cleanly unless said otherwise.
    fn answer(&self, provider: Provider) -> Answer {
        self.answers
            .iter()
            .find(|(p, _)| *p == provider)
            .map(|(_, a)| *a)
            .unwrap_or(Answer::Miss)
    }

    fn calls(&self) -> Vec<Provider> {
        self.calls.lock().unwrap().clone()
    }
}

impl Providers for Fake {
    fn fetch<'a>(
        &'a self,
        provider: Provider,
        _track: &'a Track,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<Found>>> + Send + 'a>> {
        self.calls.lock().unwrap().push(provider);
        let result = match self.answer(provider) {
            Answer::Hit => Ok(Some(found_from(provider, WORD_TIMED))),
            Answer::LineHit => Ok(Some(found_from(provider, LINE_TIMED))),
            Answer::PastEnd => Ok(Some(found_from(provider, PAST_END))),
            Answer::Miss => Ok(None),
            Answer::Boom => Err(std::io::Error::other(format!("{provider} is down")).into()),
        };
        Box::pin(async move { result })
    }
}

const WORD_TIMED: &str = "[00:01.000]<00:01.000>Hello<00:01.500> <00:02.000>world<00:02.500>\n";
const LINE_TIMED: &str = "[00:01.000]Hello world\n";
/// The track is 238.6s long; this last line starts well past four minutes.
const PAST_END: &str = "[00:01.000]Hello world\n[04:30.000]Still going\n";

fn found_from(provider: Provider, raw: &str) -> Found {
    Found {
        lyrics: lrc::parse(raw),
        source: match provider {
            Provider::Amll => Source::Amll,
            Provider::LyricsPlus => Source::LyricsPlus,
            Provider::LrcMux => Source::LrcMux {
                provider: "musixmatch".to_string(),
            },
            Provider::LrcLib => Source::LrcLib { id: 1 },
        },
        synced: true,
        raw: raw.to_string(),
    }
}

fn track() -> Track {
    Track {
        id: "https://open.spotify.com/track/70LcF31zb1H0PyJoS1Sx1r".to_string(),
        title: "Creep".to_string(),
        artist: "Radiohead".to_string(),
        album: None,
        length: Some(238.6),
    }
}

async fn chain(fake: &Fake, order: &[Provider]) -> anyhow::Result<Option<Found>> {
    terminal_lyrics::lyrics::first_hit(fake, order, &track()).await
}

const DEFAULT: [Provider; 4] = Provider::DEFAULT_ORDER;

#[tokio::test]
async fn the_documented_order_is_the_order_they_are_asked_in() {
    let fake = Fake::new(&[]);
    assert!(chain(&fake, &DEFAULT).await.unwrap().is_none());
    assert_eq!(fake.calls(), DEFAULT.to_vec());
}

#[tokio::test]
async fn the_first_hit_stops_the_chain() {
    // AMLL misses, LyricsPlus answers: lrcmux and LRCLIB are never troubled.
    let fake = Fake::new(&[(Provider::LyricsPlus, Answer::Hit)]);
    let found = chain(&fake, &DEFAULT).await.unwrap().expect("a hit");
    assert_eq!(found.source, Source::LyricsPlus);
    assert_eq!(fake.calls(), vec![Provider::Amll, Provider::LyricsPlus]);
}

#[tokio::test]
async fn a_provider_being_down_does_not_cost_the_lower_tier_answer() {
    // Both community-run services are unreachable. LRCLIB's line-level answer
    // must still arrive — this is the whole reason failures fall through.
    let fake = Fake::new(&[
        (Provider::LyricsPlus, Answer::Boom),
        (Provider::LrcMux, Answer::Boom),
        (Provider::LrcLib, Answer::Hit),
    ]);
    let found = chain(&fake, &DEFAULT).await.unwrap().expect("a hit");
    assert_eq!(found.source, Source::LrcLib { id: 1 });
    assert_eq!(
        fake.calls(),
        DEFAULT.to_vec(),
        "every provider was still asked"
    );
}

#[tokio::test]
async fn a_clean_miss_beside_a_broken_provider_is_still_a_miss() {
    // One service is down, the rest say "not here". That is real information,
    // and reporting it as a lookup failure would turn every unlyricked track
    // into an error for as long as the outage lasted.
    let fake = Fake::new(&[(Provider::LrcMux, Answer::Boom)]);
    assert!(chain(&fake, &DEFAULT).await.unwrap().is_none());
}

#[tokio::test]
async fn everything_failing_is_reported_as_a_failure_not_a_miss() {
    // Nothing answered, so nothing is known. Saying "no lyrics" here would
    // cache a lie for a day.
    let fake = Fake::new(&[
        (Provider::Amll, Answer::Boom),
        (Provider::LyricsPlus, Answer::Boom),
        (Provider::LrcMux, Answer::Boom),
        (Provider::LrcLib, Answer::Boom),
    ]);
    assert!(chain(&fake, &DEFAULT).await.is_err());
}

#[tokio::test]
async fn the_configured_order_is_obeyed_and_omitted_providers_are_skipped() {
    // `providers = ["lrcmux", "lrclib"]` is how a provider is turned off.
    let order = [Provider::LrcMux, Provider::LrcLib];
    let fake = Fake::new(&[
        (Provider::Amll, Answer::Hit),
        (Provider::LrcLib, Answer::Hit),
    ]);
    let found = chain(&fake, &order).await.unwrap().expect("a hit");
    assert_eq!(found.source, Source::LrcLib { id: 1 });
    assert_eq!(fake.calls(), order.to_vec(), "amll was configured away");
}

#[tokio::test]
async fn an_empty_list_asks_nobody() {
    let fake = Fake::new(&[(Provider::Amll, Answer::Hit)]);
    assert!(chain(&fake, &[]).await.unwrap().is_none());
    assert!(fake.calls().is_empty());
}

#[tokio::test]
async fn a_cached_answer_short_circuits_the_whole_chain() {
    // Which is why `CACHE_VERSION` has to be bumped when the chain gains a
    // better source: nothing here is reached for a track already played.
    let dir = std::env::temp_dir().join(format!("terminal-lyrics-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = Cache::new(Some(dir.clone()));
    let track = track();

    let fake = Fake::new(&[(Provider::Amll, Answer::Hit)]);
    let first = terminal_lyrics::lyrics::lookup(&fake, &DEFAULT, &cache, &track)
        .await
        .unwrap();
    assert!(matches!(first, Outcome::Found(f) if f.source == Source::Amll));
    assert_eq!(fake.calls(), vec![Provider::Amll]);

    let again = Fake::new(&[(Provider::Amll, Answer::Hit)]);
    let second = terminal_lyrics::lyrics::lookup(&again, &DEFAULT, &cache, &track)
        .await
        .unwrap();
    assert!(matches!(second, Outcome::Found(f) if f.source == Source::Cache));
    assert!(again.calls().is_empty(), "the cache was consulted first");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_line_level_answer_does_not_stop_the_chain() {
    // The case this was written for, seen live: LyricsPlus serves Apple's
    // line-level document for a minority of tracks, and lrcmux had word
    // timings for one of them. Stopping at the first *answer* rather than the
    // first good one threw those away, and nothing on screen said so.
    let fake = Fake::new(&[
        (Provider::LyricsPlus, Answer::LineHit),
        (Provider::LrcMux, Answer::Hit),
    ]);
    let found = chain(&fake, &DEFAULT).await.unwrap().expect("a hit");
    assert_eq!(
        found.source,
        Source::LrcMux {
            provider: "musixmatch".to_string()
        }
    );
    assert!(found.lyrics.has_word_timings());
    assert_eq!(
        fake.calls(),
        vec![Provider::Amll, Provider::LyricsPlus, Provider::LrcMux],
        "the chain carried on past the line-level answer, and stopped at the word-timed one"
    );
}

#[tokio::test]
async fn a_line_level_answer_is_still_returned_when_nothing_better_exists() {
    // The fallback has to actually come back. Most tracks that come back
    // line-level are line-level everywhere, and showing nothing would be a
    // plain regression against the version that stopped at the first answer.
    let fake = Fake::new(&[(Provider::LyricsPlus, Answer::LineHit)]);
    let found = chain(&fake, &DEFAULT).await.unwrap().expect("the fallback");
    assert_eq!(found.source, Source::LyricsPlus);
    assert!(!found.lyrics.has_word_timings());
    assert_eq!(
        fake.calls(),
        DEFAULT.to_vec(),
        "everyone was asked before settling"
    );
}

#[tokio::test]
async fn the_first_line_level_answer_wins_over_a_later_one() {
    // Among answers of equal quality the documented order still decides, so
    // the fallback is kept rather than overwritten.
    let fake = Fake::new(&[
        (Provider::LyricsPlus, Answer::LineHit),
        (Provider::LrcMux, Answer::LineHit),
        (Provider::LrcLib, Answer::LineHit),
    ]);
    let found = chain(&fake, &DEFAULT).await.unwrap().expect("the fallback");
    assert_eq!(found.source, Source::LyricsPlus);
}

#[tokio::test]
async fn a_line_level_answer_beside_a_broken_provider_is_not_an_error() {
    // The fallback is a real answer, so it outranks the recorded failure —
    // otherwise an outage would hide lyrics that were successfully fetched.
    let fake = Fake::new(&[
        (Provider::LyricsPlus, Answer::Boom),
        (Provider::LrcMux, Answer::Boom),
        (Provider::LrcLib, Answer::LineHit),
    ]);
    let found = chain(&fake, &DEFAULT).await.unwrap().expect("the fallback");
    assert_eq!(found.source, Source::LrcLib { id: 1 });
}

#[tokio::test]
async fn lyrics_timed_past_the_end_of_the_song_are_stepped_over() {
    // A confidently word-timed answer for a *longer* recording is not a better
    // answer than a line-level one for this one — it is wrong from the first
    // line, because the whole document is shifted. So it must not stop the
    // chain, and must not be kept as the fallback either.
    let fake = Fake::new(&[
        (Provider::LrcMux, Answer::PastEnd),
        (Provider::LrcLib, Answer::LineHit),
    ]);
    let found = chain(&fake, &DEFAULT).await.unwrap().expect("a hit");
    assert_eq!(
        found.source,
        Source::LrcLib { id: 1 },
        "LRCLIB's line-level answer for this recording beats word timings for another"
    );
    assert_eq!(fake.calls(), DEFAULT.to_vec());
}

#[tokio::test]
async fn an_overrunning_answer_counts_as_a_miss_not_an_outage() {
    // Nothing else has anything. The lookup must come back as a clean miss, so
    // it is cached as one and the UI says "no lyrics" rather than "network
    // trouble" — the provider did answer, it just answered about another edit.
    let fake = Fake::new(&[(Provider::LrcMux, Answer::PastEnd)]);
    assert!(chain(&fake, &DEFAULT).await.unwrap().is_none());
}
