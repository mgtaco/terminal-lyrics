//! The lookup itself: every provider at once, the best answer wins.
//!
//! Providers are asked concurrently, but their order still means something.
//! Sync outranks priority — word over line over plain — and among equals the
//! earlier provider wins, so a fast answer from low in the list is held until
//! everything ahead of it that could match it has finished. An answer wins as
//! soon as nothing still out can beat it; once priority alone has a winner, the
//! wait for better sync is bounded by [`SYNC_GRACE`].
//!
//! Around each provider: its cache entry first, then a rate limit, then the
//! fetch, whose answer — hit or miss — goes back into the cache.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::task::JoinSet;

use super::Provider;
use super::cache::{Cache, Entry, provider_key};
use super::error::{Error, Result};
use super::model::{Lyrics, Request};
use super::providers::{self, Http};
use super::sidecar;

/// How long a priority winner is held for better sync still in flight.
pub const SYNC_GRACE: Duration = Duration::from_secs(2);
/// The whole race.
pub const LOOKUP_TIMEOUT: Duration = Duration::from_secs(12);

const RATE_BURST: f64 = 10.0;
/// Tokens per second.
const RATE_REFILL: f64 = 0.5;

pub type FetchFuture = Pin<Box<dyn Future<Output = Result<Lyrics>> + Send + 'static>>;

/// Look up one track. Empty lyrics are a miss and get cached; an error does not.
pub trait Fetch: Send + Sync {
    fn fetch(&self, req: Request) -> FetchFuture;
}

/// One provider.
#[derive(Clone)]
pub struct Source {
    /// Whether this provider usually has word timing, which is what makes a
    /// line-synced winner worth holding for it.
    pub word_sync: bool,
    pub fetch: Arc<dyn Fetch>,
}

/// Where an answer came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Nowhere,
    Provider(Provider),
    /// A lyrics file next to the track.
    Sidecar(PathBuf),
}

/// What a lookup found. Not found is `found: false`, not an error.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub found: bool,
    pub origin: Origin,
    pub cached: bool,
    pub lyrics: Lyrics,
}

impl Answer {
    pub fn none() -> Self {
        Answer {
            found: false,
            origin: Origin::Nowhere,
            cached: false,
            lyrics: Lyrics::default(),
        }
    }

    fn cached_miss() -> Self {
        Answer {
            cached: true,
            ..Answer::none()
        }
    }

    pub fn tier(&self) -> u8 {
        self.lyrics.tier()
    }
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct Engine {
    cache: Cache,
    upgrade: bool,
    sync_grace: Duration,
    timeout: Duration,
    sources: HashMap<Provider, Source>,
    rate: Mutex<HashMap<Provider, Bucket>>,
}

struct Builtin {
    provider: Provider,
    http: Arc<Http>,
}

impl Fetch for Builtin {
    fn fetch(&self, req: Request) -> FetchFuture {
        let http = self.http.clone();
        let provider = self.provider;
        Box::pin(async move {
            match provider {
                Provider::BetterLyrics => providers::betterlyrics::fetch(&http, &req).await,
                Provider::Unison => providers::unison::fetch(&http, &req).await,
                Provider::LyricsPlus => providers::lyricsplus::fetch(&http, &req).await,
                Provider::KuGou => providers::kugou::fetch(&http, &req).await,
                Provider::LrcLib => providers::lrclib::fetch(&http, &req).await,
                Provider::YouTubeMusic => providers::youtubemusic::fetch(&http, &req).await,
            }
        })
    }
}

impl Engine {
    /// The built-in providers over one HTTP client.
    pub fn new(cache: Cache) -> Result<Self> {
        let http = Arc::new(Http::new()?);
        let sources = Provider::DEFAULT_ORDER
            .into_iter()
            .map(|provider| {
                let source = Source {
                    // LRCLIB has word sync too rarely to wait for.
                    word_sync: provider.word_sync(),
                    fetch: Arc::new(Builtin {
                        provider,
                        http: http.clone(),
                    }),
                };
                (provider, source)
            })
            .collect();
        Ok(Self::with_sources(cache, sources))
    }

    /// Any set of providers: the seam the race is tested through.
    pub fn with_sources(cache: Cache, sources: HashMap<Provider, Source>) -> Self {
        Engine {
            cache,
            upgrade: true,
            sync_grace: SYNC_GRACE,
            timeout: LOOKUP_TIMEOUT,
            sources,
            rate: Mutex::new(HashMap::new()),
        }
    }

    /// Shorter waits, for tests that would otherwise sit through the real ones.
    pub fn with_timing(mut self, sync_grace: Duration, timeout: Duration) -> Self {
        self.sync_grace = sync_grace;
        self.timeout = timeout;
        self
    }

    /// Stop line-synced cache entries being asked again for word sync.
    pub fn without_upgrade(mut self) -> Self {
        self.upgrade = false;
        self
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    fn ordered(&self, providers: &[Provider]) -> Result<Vec<(Provider, Source)>> {
        let mut ordered: Vec<(Provider, Source)> = Vec::with_capacity(providers.len());
        for &provider in providers {
            if ordered.iter().any(|(p, _)| *p == provider) {
                continue;
            }
            let source = self
                .sources
                .get(&provider)
                .ok_or_else(|| Error::invalid(format!("unknown lyrics provider: {provider}")))?;
            ordered.push((provider, source.clone()));
        }
        Ok(ordered)
    }

    /// Sidecar files, then the cache, then the providers in `providers`, in
    /// priority order.
    pub async fn lookup(self: &Arc<Self>, req: &Request, providers: &[Provider]) -> Result<Answer> {
        let answer = self.lookup_inner(req, providers).await;
        self.cache.prune();
        answer
    }

    async fn lookup_inner(
        self: &Arc<Self>,
        req: &Request,
        providers: &[Provider],
    ) -> Result<Answer> {
        if let Some(answer) = sidecar_answer(req) {
            return Ok(answer);
        }
        if req.artist.is_empty() || req.title.is_empty() {
            return Ok(Answer::none());
        }
        let ordered = self.ordered(providers)?;
        if ordered.is_empty() {
            return Ok(Answer::none());
        }
        self.race(ordered, req).await
    }

    /// What the cache alone says, without waiting on anything: the best entry
    /// any of `providers` holds. `None` is "not cached", not "no lyrics".
    pub fn cached(&self, req: &Request, providers: &[Provider]) -> Option<Answer> {
        if let Some(answer) = sidecar_answer(req) {
            return Some(answer);
        }
        if req.artist.is_empty() || req.title.is_empty() {
            return None;
        }
        let ordered = self.ordered(providers).ok()?;
        let mut best: Option<Answer> = None;
        for (provider, _) in ordered {
            let Some(entry) = self.cache.load(&provider_key(provider, req)) else {
                continue;
            };
            let answer = from_cache(entry, provider);
            if answer.found && best.as_ref().is_none_or(|b| answer.tier() > b.tier()) {
                best = Some(answer);
            }
        }
        best
    }

    /// Drop every provider's cache entry for the track, so the next lookup
    /// asks again.
    pub fn forget(&self, req: &Request, providers: &[Provider]) {
        for &provider in providers {
            self.cache.remove(&provider_key(provider, req));
        }
    }

    async fn race(
        self: &Arc<Self>,
        providers: Vec<(Provider, Source)>,
        req: &Request,
    ) -> Result<Answer> {
        let count = providers.len();
        let word_sync: Vec<bool> = providers.iter().map(|(_, s)| s.word_sync).collect();
        // Dropping the set aborts whatever is still out once there is a winner.
        let mut tasks = JoinSet::new();
        for (index, (provider, source)) in providers.into_iter().enumerate() {
            let engine = self.clone();
            let req = req.clone();
            tasks.spawn(
                async move { (index, engine.lookup_provider(provider, &source, &req).await) },
            );
        }

        let mut finished = vec![false; count];
        let mut results: Vec<Option<Result<Answer>>> = (0..count).map(|_| None).collect();
        let mut remaining = count;
        let deadline = tokio::time::sleep(self.timeout);
        tokio::pin!(deadline);
        let mut grace: Option<Pin<Box<tokio::time::Sleep>>> = None;

        while remaining > 0 {
            tokio::select! {
                _ = &mut deadline => {
                    // Higher-priority providers were still out; take the best
                    // that did answer.
                    return match best_found(&results) {
                        Some((best, _)) => Ok(best.clone()),
                        None => Err(Error::Timeout),
                    };
                }
                _ = async { grace.as_mut().expect("guarded").await }, if grace.is_some() => {
                    return Ok(best_found(&results).map(|(b, _)| b.clone()).unwrap_or_else(Answer::none));
                }
                joined = tasks.join_next() => {
                    remaining -= 1;
                    match joined {
                        Some(Ok((index, result))) => {
                            finished[index] = true;
                            results[index] = Some(result);
                        }
                        // A provider that panicked counts as one that never answered.
                        Some(Err(_)) => continue,
                        None => break,
                    }
                }
            }
            let Some((best, index)) = best_found(&results) else {
                continue;
            };
            let tier = best.tier();
            if !blocked_ahead(&word_sync, &finished, index, tier)
                && !upgrade_pending(&word_sync, &finished, index, tier)
            {
                return Ok(best.clone());
            }
            if grace.is_none() && leader_found(&finished, &results) {
                grace = Some(Box::pin(tokio::time::sleep(self.sync_grace)));
            }
        }

        // A provider that says "no lyrics" outranks one that merely failed.
        let mut last_error = None;
        for result in results.into_iter().flatten() {
            match result {
                Ok(_) => return Ok(Answer::none()),
                Err(e) => last_error = Some(e),
            }
        }
        Err(last_error.unwrap_or(Error::NotFound))
    }

    async fn lookup_provider(
        &self,
        provider: Provider,
        source: &Source,
        req: &Request,
    ) -> Result<Answer> {
        let key = provider_key(provider, req);
        if let Some(mut entry) = self.cache.load(&key) {
            if self.upgrade && !req.cache_only && entry.awaits_word_sync() {
                self.upgrade_entry(&key, provider, source, req, &mut entry)
                    .await;
            }
            return Ok(from_cache(entry, provider));
        }
        if req.cache_only {
            return Ok(Answer::none());
        }
        if !self.allow_request(provider) {
            return Err(Error::RateLimited);
        }
        match source.fetch.fetch(req.clone()).await {
            Err(Error::Uncached) => Ok(Answer::none()),
            Err(e) if !e.is_not_found() => Err(e),
            Err(_) => {
                self.store_miss(&key);
                Ok(Answer::none())
            }
            Ok(lyrics) if lyrics.is_empty() => {
                self.store_miss(&key);
                Ok(Answer::none())
            }
            Ok(lyrics) => {
                let mut entry = Entry {
                    source: provider.name().to_string(),
                    lyrics: lyrics.clone(),
                    ..Default::default()
                };
                self.cache.store(&key, &mut entry);
                Ok(Answer {
                    found: true,
                    origin: Origin::Provider(provider),
                    cached: false,
                    lyrics,
                })
            }
        }
    }

    fn store_miss(&self, key: &str) {
        let mut entry = Entry {
            miss: true,
            ..Default::default()
        };
        self.cache.store(key, &mut entry);
    }

    /// Swap a line-synced entry for word sync once the provider has it. The
    /// entry is restamped either way, so the provider is asked once per
    /// upgrade period rather than on every play.
    async fn upgrade_entry(
        &self,
        key: &str,
        provider: Provider,
        source: &Source,
        req: &Request,
        entry: &mut Entry,
    ) {
        if !self.allow_request(provider) {
            return;
        }
        match source.fetch.fetch(req.clone()).await {
            Err(e) if !e.is_not_found() => return,
            Ok(found) if found.word_synced() => entry.lyrics = found,
            _ => {}
        }
        self.cache.store(key, entry);
    }

    fn allow_request(&self, provider: Provider) -> bool {
        let mut buckets = self.rate.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let bucket = buckets.entry(provider).or_insert(Bucket {
            tokens: RATE_BURST,
            last: now,
        });
        let refill = now.duration_since(bucket.last).as_secs_f64() * RATE_REFILL;
        bucket.tokens = (bucket.tokens + refill).min(RATE_BURST);
        bucket.last = now;
        if bucket.tokens < 1.0 {
            return false;
        }
        bucket.tokens -= 1.0;
        true
    }
}

fn sidecar_answer(req: &Request) -> Option<Answer> {
    let (path, lyrics) = sidecar::find(&req.file_url)?;
    Some(Answer {
        found: true,
        origin: Origin::Sidecar(path),
        cached: false,
        lyrics,
    })
}

fn from_cache(entry: Entry, provider: Provider) -> Answer {
    if entry.miss {
        return Answer::cached_miss();
    }
    Answer {
        found: true,
        origin: Origin::Provider(entry.source.parse().unwrap_or(provider)),
        cached: true,
        lyrics: entry.lyrics,
    }
}

/// The highest sync tier found so far; priority order breaks ties.
fn best_found(results: &[Option<Result<Answer>>]) -> Option<(&Answer, usize)> {
    let mut best: Option<(&Answer, usize)> = None;
    for (i, result) in results.iter().enumerate() {
        let Some(Ok(answer)) = result else { continue };
        if !answer.found {
            continue;
        }
        if best.is_none_or(|(b, _)| answer.tier() > b.tier()) {
            best = Some((answer, i));
        }
    }
    best
}

/// A higher-priority provider still out that could answer at the same tier.
fn blocked_ahead(word_sync: &[bool], finished: &[bool], index: usize, tier: u8) -> bool {
    (0..index).any(|i| !finished[i] && (tier < 2 || word_sync[i]))
}

/// A lower-priority provider still out that could answer at a higher tier.
fn upgrade_pending(word_sync: &[bool], finished: &[bool], index: usize, tier: u8) -> bool {
    (index + 1..finished.len()).any(|i| !finished[i] && (tier == 0 || (tier == 1 && word_sync[i])))
}

/// Whether priority alone already has a winner: everything ahead of the first
/// hit has finished.
fn leader_found(finished: &[bool], results: &[Option<Result<Answer>>]) -> bool {
    for (i, result) in results.iter().enumerate() {
        if !finished[i] {
            return false;
        }
        if let Some(Ok(answer)) = result
            && answer.found
        {
            return true;
        }
    }
    false
}
