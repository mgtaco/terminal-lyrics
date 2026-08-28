//! Windows, via the System Media Transport Controls.
//!
//! SMTC is the API behind the media flyout in the volume panel: every app that
//! wants its track shown there registers a session, which makes it the closest
//! thing Windows has to MPRIS. Unlike AppleScript on macOS it is a real
//! discovery API, so this backend does not need to know the players by name —
//! whatever registers a session can be followed.
//!
//! What it does not report is any stable identifier for the track. There is no
//! `spotify:track:<id>` here the way there is in Spotify's AppleScript
//! dictionary and its MPRIS metadata, so [`Track::id`] falls back to
//! artist-and-title. That costs exactly one provider: [`crate::lyrics::amll`] is
//! keyed by Spotify ID and will be stepped over. `lyricsplus` and `lrcmux` match
//! on artist and title, so word-by-word lyrics — the point of the program —
//! still work here.
//!
//! Two things are worth knowing about the edge:
//!
//! * WinRT wants a multithreaded apartment, and its async operations complete
//!   on whichever thread the runtime picks rather than one we control. See
//!   [`ensure_mta`].
//! * A WinRT `TimeSpan` counts 100-nanosecond ticks, not milliseconds and not
//!   seconds. Getting that wrong does not fail — it silently reports a position
//!   that is out by a factor of ten million, which looks like a sync bug rather
//!   than a unit bug.

use std::sync::Once;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use tokio::sync::mpsc;

use super::{EventRx, EventTx, PlayerEvent, PlayerState, Snapshot, Track, fallback_id};

use ::windows::Media::Control::GlobalSystemMediaTransportControlsSession as SmtcSession;
use ::windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as SmtcManager;
use ::windows::Win32::System::Com::CoIncrementMTAUsage;

/// How long one read of the session list may take before we give up on it.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Consecutive failed probes before the player is treated as gone. Matching the
/// macOS backend: a single unlucky read is not a reason to tear the view down.
const MISSES_BEFORE_GONE: u32 = 3;

/// A WinRT `TimeSpan` is a count of 100-nanosecond ticks.
const TICKS_PER_SECOND: f64 = 10_000_000.0;

/// `GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing`.
///
/// The enum's other values are Closed, Opened, Changing, Stopped and Paused, in
/// that order from zero. Only one of them means sound is coming out.
const STATUS_PLAYING: i32 = 5;

/// One SMTC session, read out of WinRT into plain owned values.
///
/// Nothing in here is a WinRT type, and that is the whole point: every decision
/// this backend makes happens in [`parse_sessions`] against this struct, so the
/// decisions are testable without a media session, and the WinRT code below is
/// left as a transcription with no judgement in it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawSession {
    /// `SourceAppUserModelId`, e.g. `Spotify.exe`.
    pub app_id: String,
    /// `PlaybackStatus` as its underlying integer.
    pub status: i32,
    pub position_ticks: i64,
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

/// Turn a `SourceAppUserModelId` into the short name the user types.
///
/// Desktop apps register an executable (`Spotify.exe`); packaged apps register a
/// full AUMID (`Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic`). Both
/// have to come out as something a person would guess, lowercased so it matches
/// the short names the other backends use — `spotify` on Linux is `spotify`
/// here too, and `--player spot` finds it through [`super::match_name`].
pub fn friendly_name(app_id: &str) -> String {
    let s = app_id.trim();
    // A packaged app's AUMID is `<package family>!<app id>`; the half after the
    // bang is the specific one.
    let s = s.rsplit('!').next().unwrap_or(s);
    // `Spotify.exe` -> `Spotify`. Done before the dot split below, which would
    // otherwise reduce it to `exe`.
    //
    // Cutting at the last dot rather than at `len - 4`: an app id is arbitrary
    // text, and `len - 4` can land in the middle of a multi-byte character,
    // where slicing panics. `rfind` only ever returns a character boundary.
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

/// Everything this backend decides, in one pure function.
///
/// A session with no title is kept but carries no track: a browser playing a
/// video it has no metadata for is still a player, it just is not one with
/// lyrics, and [`super::rank_players`] already prefers a session that has a
/// track over one that does not.
pub fn parse_sessions(raw: &[RawSession]) -> Vec<Probe> {
    let mut out: Vec<Probe> = Vec::new();

    for r in raw {
        let title = r.title.trim();
        let artist = r.artist.trim();
        let album = r.album.trim();

        let track = (!title.is_empty()).then(|| Track {
            // SMTC exposes no track id, so this is the shared last-resort key —
            // the same one the other backends fall back to, so a track keyed
            // here hits the same cache entry it would on Linux.
            id: fallback_id(artist, title),
            title: title.to_string(),
            artist: artist.to_string(),
            album: (!album.is_empty()).then(|| album.to_string()),
            // A live stream reports no end time; zero means "unknown", not
            // "zero seconds long", and a bogus length would put every LRCLIB
            // candidate outside the duration window.
            length: (r.end_ticks > 0).then(|| seconds(r.end_ticks)),
        });

        let probe = Probe {
            name: friendly_name(&r.app_id),
            snapshot: Snapshot {
                playing: r.status == STATUS_PLAYING,
                position: seconds(r.position_ticks),
                track,
            },
        };

        // Two windows of the same browser register two sessions. Keeping the
        // better of them stops `--player msedge` being a coin flip between the
        // tab playing music and the one that is only holding a paused video.
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

/// Register the process as a multithreaded apartment.
///
/// WinRT needs COM initialised, and the calls below resolve on whichever thread
/// the runtime completes them on rather than one we control.
/// `CoIncrementMTAUsage` sets the process up once and, unlike `CoInitializeEx`,
/// has no matching uninit to pair with — which is what suits a process that
/// never wants to tear the apartment back down.
///
/// The cookie it returns is only an argument for `CoDecrementMTAUsage`, which
/// is never called here. It is `Copy` and so carries no destructor, meaning
/// dropping it does nothing and the apartment stays up regardless.
fn ensure_mta() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = unsafe { CoIncrementMTAUsage() };
    });
}

/// Every live session, as an owned `Vec`.
///
/// The `Vec` is the point. `GetSessions` hands back an `IVectorView`, and WinRT
/// collections are bound to the apartment that made them, so windows-rs does not
/// mark them `Send` — hold one across an `.await` and the whole future stops
/// being `Send`, which `tokio::spawn` requires of the pump. The sessions inside
/// it are agile and `Send`, so draining the view here and dropping it before any
/// await is what keeps the async path usable.
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

/// The parts of a session that read synchronously.
///
/// Split out so that none of the apartment-bound property objects are still
/// alive when [`read_sessions`] awaits: same reasoning as [`live_sessions`].
fn read_sync(session: &SmtcSession) -> Option<(String, i32, i64, i64)> {
    let app_id = session.SourceAppUserModelId().ok()?.to_string();
    let status = session.GetPlaybackInfo().ok()?.PlaybackStatus().ok()?.0;
    let (position_ticks, end_ticks) = match session.GetTimelineProperties() {
        Ok(t) => (
            t.Position().map(|p| p.Duration).unwrap_or(0),
            t.EndTime().map(|e| e.Duration).unwrap_or(0),
        ),
        Err(_) => (0, 0),
    };
    Some((app_id, status, position_ticks, end_ticks))
}

/// Read every session out of WinRT.
///
/// WinRT's async operations are futures here, not blocking calls: windows-rs
/// gives `IAsyncOperation` an `IntoFuture`, so this awaits on the normal runtime
/// rather than occupying a blocking thread.
async fn read_sessions() -> Result<Vec<RawSession>> {
    let sessions = live_sessions().await?;

    let mut out = Vec::with_capacity(sessions.len());
    for session in sessions {
        let Some((app_id, status, position_ticks, end_ticks)) = read_sync(&session) else {
            continue;
        };

        // The media properties are a second async call and the likeliest to
        // fail. A session without them is still a player worth reporting; it
        // just has no track yet.
        let props = match session.TryGetMediaPropertiesAsync() {
            Ok(op) => op.await.ok(),
            Err(_) => None,
        };
        let (title, artist, album) = match props {
            Some(p) => (
                p.Title().map(|h| h.to_string()).unwrap_or_default(),
                p.Artist().map(|h| h.to_string()).unwrap_or_default(),
                p.AlbumTitle().map(|h| h.to_string()).unwrap_or_default(),
            ),
            None => (String::new(), String::new(), String::new()),
        };

        out.push(RawSession {
            app_id,
            status,
            position_ticks,
            end_ticks,
            title,
            artist,
            album,
        });
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

/// Run one probe, with a deadline.
async fn probe() -> Result<Vec<Probe>> {
    let raw = tokio::time::timeout(PROBE_TIMEOUT, read_sessions())
        .await
        .map_err(|_| {
            anyhow!("the media session manager did not answer within {PROBE_TIMEOUT:?}")
        })??;
    Ok(parse_sessions(&raw))
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

    /// Spawn the event pump.
    ///
    /// Polling, like macOS. SMTC does expose change events, but they are an
    /// optimisation rather than a requirement: [`crate::sync::SyncEngine`]'s
    /// `Tick` arm already re-anchors on divergence, which is how a scrub is
    /// picked up on the other two backends as well.
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
                // The probe itself failed. That is not the same as the player
                // being gone, and one unlucky read is a poor reason to tear the
                // visualiser down.
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
