//! System Media Transport Controls. WinRT wants a multithreaded apartment (see
//! [`ensure_mta`]), and a `TimeSpan` counts 100-nanosecond ticks.

use std::sync::Once;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use tokio::sync::mpsc;

use super::{EventRx, EventTx, PlayerEvent, PlayerState, Snapshot, Track, fallback_id};

use ::windows::Media::Control::GlobalSystemMediaTransportControlsSession as SmtcSession;
use ::windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as SmtcManager;
use ::windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus;
use ::windows::Win32::System::Com::CoIncrementMTAUsage;

/// How long one read of the session list may take before we give up on it.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Consecutive failed probes before the player is treated as gone. Matching the
/// macOS backend: a single unlucky read is not a reason to tear the view down.
const MISSES_BEFORE_GONE: u32 = 3;

/// A WinRT `TimeSpan` is a count of 100-nanosecond ticks.
const TICKS_PER_SECOND: f64 = 10_000_000.0;

const STATUS_PLAYING: i32 = PlaybackStatus::Playing.0;

/// Plain owned values, so every decision happens in [`parse_sessions`] and can
/// be tested without a media session.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawSession {
    /// `SourceAppUserModelId`, e.g. `Spotify.exe`.
    pub app_id: String,
    /// `PlaybackStatus` as its underlying integer.
    pub status: i32,
    pub position_ticks: i64,
    /// `LastUpdatedTime` as a `DateTime`: when `position_ticks` was true.
    pub updated_ticks: i64,
    pub end_ticks: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
}

/// One player, named the way a user would type it after `--player`.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub name: String,
    pub snapshot: Snapshot,
}

/// `Spotify.exe` or `Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic`,
/// lowercased to match the other backends' short names.
pub fn friendly_name(app_id: &str) -> String {
    let s = app_id.trim();
    // A packaged app's AUMID is `<package family>!<app id>`; the half after the
    // bang is the specific one.
    let s = s.rsplit('!').next().unwrap_or(s);
    // Cut at the last dot, not `len - 4`, which can land mid-character and
    // panic. Before the dot split below, which would leave `exe`.
    let s = match s.rfind('.') {
        Some(dot) if s[dot..].eq_ignore_ascii_case(".exe") => &s[..dot],
        _ => s,
    };
    // `Microsoft.ZuneMusic` -> `ZuneMusic`.
    let s = s.rsplit('.').next().unwrap_or(s);
    // `ZuneMusic_8wekyb3d8bbwe` -> `ZuneMusic`, for an AUMID with no bang in it.
    let s = s.split('_').next().unwrap_or(s);

    let name = s.trim().to_lowercase();
    // Something unrecognisable is still a player worth listing, so fall back to
    // the raw id rather than dropping the session.
    if name.is_empty() {
        app_id.trim().to_lowercase()
    } else {
        name
    }
}

fn seconds(ticks: i64) -> f64 {
    (ticks as f64 / TICKS_PER_SECOND).max(0.0)
}

/// How good a candidate a session is, for picking between two of the same name.
fn score(snap: &Snapshot) -> u8 {
    let has_track = snap.track.as_ref().is_some_and(|t| t.is_usable());
    match (snap.playing, has_track) {
        (true, true) => 3,
        (false, true) => 2,
        (true, false) => 1,
        (false, false) => 0,
    }
}

/// A session with no title is kept without a track: a browser playing an
/// untitled video is still a player, just one without lyrics.
pub fn parse_sessions(raw: &[RawSession], now_ticks: i64) -> Vec<Probe> {
    let mut out: Vec<Probe> = Vec::new();

    for r in raw {
        let title = r.title.trim();
        let artist = r.artist.trim();
        let album = r.album.trim();

        let track = (!title.is_empty()).then(|| Track {
            // SMTC has no track id; the shared last-resort key lets a track hit
            // the same cache entry it would on Linux.
            id: fallback_id(artist, title),
            title: title.to_string(),
            artist: artist.to_string(),
            album: (!album.is_empty()).then(|| album.to_string()),
            // A live stream reports no end time; a bogus length would put every
            // candidate outside the duration window.
            length: (r.end_ticks > 0).then(|| seconds(r.end_ticks)),
            url: None,
        });

        let playing = r.status == STATUS_PLAYING;
        // The position is as of `updated_ticks`, which Spotify moves only every
        // few seconds while playing (and keeps moving while paused).
        let age = if playing && r.updated_ticks > 0 {
            (now_ticks - r.updated_ticks).max(0)
        } else {
            0
        };
        let probe = Probe {
            name: friendly_name(&r.app_id),
            snapshot: Snapshot {
                playing,
                position: seconds(r.position_ticks + age),
                track,
            },
        };

        // Two windows of one browser register two sessions; keep the better, or
        // `--player msedge` is a coin flip.
        match out.iter_mut().find(|p| p.name == probe.name) {
            Some(existing) if score(&probe.snapshot) > score(&existing.snapshot) => {
                *existing = probe;
            }
            Some(_) => {}
            None => out.push(probe),
        }
    }

    out
}

/// `CoIncrementMTAUsage` rather than `CoInitializeEx`: it needs no matching
/// uninit, and dropping its `Copy` cookie leaves the apartment up.
fn ensure_mta() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = unsafe { CoIncrementMTAUsage() };
    });
}

/// An owned `Vec`: WinRT collections are not `Send`, and holding one across an
/// `.await` would stop the pump's future being spawnable.
async fn live_sessions() -> Result<Vec<SmtcSession>> {
    ensure_mta();

    let manager = SmtcManager::RequestAsync()
        .context("the Windows media session manager is unavailable")?
        .await
        .context("could not open the Windows media session manager")?;
    let view = manager
        .GetSessions()
        .context("could not list the Windows media sessions")?;

    // One unreadable session — an app shutting down mid-read — must not hide
    // the others, so each is taken on its own.
    Ok((0..view.Size().unwrap_or(0))
        .filter_map(|i| view.GetAt(i).ok())
        .collect())
}

/// Synchronous reads, so no apartment-bound object is alive when
/// [`read_sessions`] awaits.
fn read_sync(session: &SmtcSession) -> Option<RawSession> {
    let app_id = session.SourceAppUserModelId().ok()?.to_string();
    let status = session.GetPlaybackInfo().ok()?.PlaybackStatus().ok()?.0;
    let (position_ticks, updated_ticks, end_ticks) = match session.GetTimelineProperties() {
        Ok(t) => (
            t.Position().map(|p| p.Duration).unwrap_or(0),
            t.LastUpdatedTime().map(|d| d.UniversalTime).unwrap_or(0),
            t.EndTime().map(|e| e.Duration).unwrap_or(0),
        ),
        Err(_) => (0, 0, 0),
    };
    Some(RawSession {
        app_id,
        status,
        position_ticks,
        updated_ticks,
        end_ticks,
        ..RawSession::default()
    })
}

async fn read_sessions() -> Result<Vec<RawSession>> {
    let sessions = live_sessions().await?;

    let mut out = Vec::with_capacity(sessions.len());
    for session in sessions {
        let Some(mut raw) = read_sync(&session) else {
            continue;
        };

        // The media properties are the likeliest call to fail; a session
        // without them is still a player, just with no track yet.
        let props = match session.TryGetMediaPropertiesAsync() {
            Ok(op) => op.await.ok(),
            Err(_) => None,
        };
        if let Some(p) = props {
            raw.title = p.Title().map(|h| h.to_string()).unwrap_or_default();
            raw.artist = p.Artist().map(|h| h.to_string()).unwrap_or_default();
            raw.album = p.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
        }
        out.push(raw);
    }

    Ok(out)
}

/// Toggle play/pause on the named session.
async fn toggle(name: &str) -> Result<()> {
    for session in live_sessions().await? {
        // Owned `String`, not the `HSTRING`: only `Send` values may still be
        // alive at the await below.
        let Some(app_id) = session.SourceAppUserModelId().ok().map(|h| h.to_string()) else {
            continue;
        };
        if friendly_name(&app_id) == name {
            session
                .TryTogglePlayPauseAsync()
                .context("play/pause was refused")?
                .await
                .context("play/pause did not complete")?;
            return Ok(());
        }
    }

    Err(anyhow!("{name} is no longer registered as a media session"))
}

/// The wall clock as a WinRT `DateTime`: 100-nanosecond ticks since 1601.
fn now_ticks() -> i64 {
    const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
    let since_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    UNIX_EPOCH_TICKS + (since_unix.as_nanos() / 100) as i64
}

/// Run one probe, with a deadline.
async fn probe() -> Result<Vec<Probe>> {
    let raw = tokio::time::timeout(PROBE_TIMEOUT, read_sessions())
        .await
        .map_err(|_| {
            anyhow!("the media session manager did not answer within {PROBE_TIMEOUT:?}")
        })??;
    Ok(parse_sessions(&raw, now_ticks()))
}

/// Nothing to hold open: the session manager is asked for afresh each probe.
pub struct Session;

impl Session {
    pub async fn open() -> Result<Self> {
        // Fail here, at startup, rather than at the first poll, so an
        // unsupported build of Windows says so plainly.
        probe().await?;
        Ok(Self)
    }

    pub async fn survey(&self) -> Result<Vec<PlayerState>> {
        Ok(probe()
            .await?
            .into_iter()
            .map(|p| PlayerState {
                name: p.name,
                playing: p.snapshot.playing,
                has_track: p.snapshot.track.is_some_and(|t| t.is_usable()),
            })
            .collect())
    }

    pub async fn resolve(&self, wanted: Option<&str>) -> Result<String> {
        super::choose(
            self.survey().await?,
            wanted,
            "app registered with Windows' media controls",
        )
    }

    pub async fn connect(&self, name: &str) -> Result<PlayerHandle> {
        Ok(PlayerHandle {
            name: name.to_string(),
        })
    }
}

pub struct PlayerHandle {
    name: String,
}

impl PlayerHandle {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn snapshot(&self) -> Option<Snapshot> {
        probe()
            .await
            .ok()?
            .into_iter()
            .find(|p| p.name == self.name)
            .map(|p| p.snapshot)
    }

    pub async fn track(&self) -> Option<Track> {
        self.snapshot().await?.track
    }

    pub async fn position(&self) -> Option<f64> {
        self.snapshot().await.map(|s| s.position)
    }

    pub async fn playing(&self) -> bool {
        self.snapshot().await.is_some_and(|s| s.playing)
    }

    pub async fn play_pause(&self) -> Result<()> {
        toggle(&self.name).await
    }

    /// Polling, like macOS: the `Tick` arm already re-anchors on divergence, so
    /// SMTC's change events are not needed.
    pub fn spawn(self, poll_interval: Duration) -> (EventRx, tokio::task::JoinHandle<()>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = tokio::spawn(async move {
            self.pump(tx, poll_interval).await;
        });
        (rx, handle)
    }

    async fn pump(self, tx: EventTx, poll_interval: Duration) {
        let mut ticker = tokio::time::interval(poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut last_id: Option<String> = None;
        let mut primed = false;
        let mut misses = 0u32;

        loop {
            // `interval` fires immediately the first time, so the consumer is
            // primed on this tick rather than a poll interval late.
            ticker.tick().await;

            let snap = match probe().await {
                // The probe ran and our session was not in it: the app has
                // deregistered, which for SMTC means it has closed or stopped.
                Ok(rows) => match rows.into_iter().find(|p| p.name == self.name) {
                    Some(row) => row.snapshot,
                    None => {
                        let _ = tx.send(PlayerEvent::Gone);
                        return;
                    }
                },
                // The probe itself failed; one unlucky read is no reason to
                // tear the view down.
                Err(_) => {
                    misses += 1;
                    if misses >= MISSES_BEFORE_GONE {
                        let _ = tx.send(PlayerEvent::Gone);
                        return;
                    }
                    continue;
                }
            };
            misses = 0;

            let id = snap.track.as_ref().map(|t| t.id.clone());
            if !primed || id != last_id {
                last_id = id;
                primed = true;
                if tx
                    .send(PlayerEvent::Track(snap.track.map(Box::new)))
                    .is_err()
                {
                    return;
                }
                // A new track has just reset the position, so anchor on it
                // rather than waiting for the drift check to notice.
                if tx
                    .send(PlayerEvent::Status {
                        playing: snap.playing,
                        position: snap.position,
                    })
                    .is_err()
                {
                    return;
                }
                continue;
            }

            if tx
                .send(PlayerEvent::Tick {
                    position: snap.position,
                    playing: snap.playing,
                })
                .is_err()
            {
                return;
            }
        }
    }
}
