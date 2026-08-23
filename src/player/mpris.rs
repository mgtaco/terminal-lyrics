//! MPRIS over D-Bus, via zbus. No subprocesses.
//!
//! Two things about real players that a spec-literal decoder gets wrong, both
//! confirmed against the Spotify client on this machine:
//!
//! ```text
//! mpris:length   spec says x (i64)   Spotify sends t (u64)
//! mpris:trackid  spec says o (path)  Spotify sends s (string)
//! ```
//!
//! So every metadata value is read leniently, by trying the plausible variant
//! types in turn. A strict decoder gets `None` for the track length, which in
//! turn ruins the LRCLIB duration match.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use tokio::sync::mpsc;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, proxy};

use super::{EventRx, EventTx, PlayerEvent, Track};

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";

#[proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2"
)]
pub trait MprisPlayer {
    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<HashMap<String, OwnedValue>>;

    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn rate(&self) -> zbus::Result<f64>;

    /// Deliberately uncached: MPRIS declares `Position` as never emitting a
    /// change signal, so each read must hit the bus.
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> zbus::Result<i64>;

    #[zbus(signal)]
    fn seeked(&self, position: i64) -> zbus::Result<()>;

    fn play_pause(&self) -> zbus::Result<()>;
}

/// Microseconds on the wire, seconds everywhere in this program.
fn us_to_secs(us: i64) -> f64 {
    us as f64 / 1_000_000.0
}

/// Read an integer that different players type differently (`t`, `x`, `u`, `i`,
/// or even a stringified number).
fn lenient_i64(v: &OwnedValue) -> Option<i64> {
    if let Ok(x) = i64::try_from(v) {
        return Some(x);
    }
    if let Ok(x) = u64::try_from(v) {
        return i64::try_from(x).ok();
    }
    if let Ok(x) = u32::try_from(v) {
        return Some(x as i64);
    }
    if let Ok(x) = i32::try_from(v) {
        return Some(x as i64);
    }
    if let Ok(x) = f64::try_from(v) {
        return Some(x as i64);
    }
    lenient_string(v).and_then(|s| s.trim().parse().ok())
}

/// Read a string that may be `s`, an object path `o`, or a signature.
fn lenient_string(v: &OwnedValue) -> Option<String> {
    if let Ok(s) = <&str>::try_from(v) {
        return Some(s.to_string());
    }
    if let Ok(s) = String::try_from(v.clone()) {
        return Some(s);
    }
    if let Ok(p) = zbus::zvariant::ObjectPath::try_from(v.clone()) {
        return Some(p.as_str().to_string());
    }
    None
}

/// `xesam:artist` is `as`, but some players send a bare `s`.
fn lenient_string_list(v: &OwnedValue) -> Vec<String> {
    if let Ok(list) = Vec::<String>::try_from(v.clone()) {
        return list;
    }
    lenient_string(v).into_iter().collect()
}

/// Build a [`Track`] from an MPRIS metadata map.
pub fn track_from_metadata(md: &HashMap<String, OwnedValue>) -> Option<Track> {
    let title = md.get("xesam:title").and_then(lenient_string)?;

    let artists = md
        .get("xesam:artist")
        .map(lenient_string_list)
        .unwrap_or_default();
    let artist = artists.first().cloned().unwrap_or_default();

    let album = md
        .get("xesam:album")
        .and_then(lenient_string)
        .filter(|s| !s.trim().is_empty());

    // `t` on Spotify, `x` per spec; treat non-positive as absent.
    let length = md
        .get("mpris:length")
        .and_then(lenient_i64)
        .filter(|&us| us > 0)
        .map(us_to_secs);

    let url = md
        .get("xesam:url")
        .and_then(lenient_string)
        .filter(|s| !s.is_empty());
    // `s` on Spotify, `o` per spec.
    let trackid = md.get("mpris:trackid").and_then(lenient_string);
    let id = url
        .or(trackid)
        .unwrap_or_else(|| format!("{artist}\u{1}{title}"));

    Some(Track {
        id,
        title,
        artist,
        album,
        length,
    })
}

fn status_is_playing(s: &str) -> bool {
    s.eq_ignore_ascii_case("playing")
}

/// Every MPRIS name currently on the session bus, e.g. `spotify`, `vlc`.
pub async fn list_players(conn: &Connection) -> Result<Vec<String>> {
    let dbus = zbus::fdo::DBusProxy::new(conn)
        .await
        .context("failed to open the D-Bus daemon proxy")?;
    let mut names: Vec<String> = dbus
        .list_names()
        .await
        .context("failed to list bus names")?
        .into_iter()
        .filter_map(|n| {
            n.as_str()
                .strip_prefix(MPRIS_PREFIX)
                .map(|s| s.to_string())
        })
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// What a candidate player looks like right now, for ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerState {
    pub name: String,
    pub playing: bool,
    pub has_track: bool,
}

/// Choose between players when the user has not named one.
///
/// Taking the first name alphabetically is wrong in practice: a browser
/// registers an idle `org.mpris.MediaPlayer2.chromium.instance…` that sorts
/// before `spotify` and reports no track at all, so the visualiser would follow
/// silence while music played next to it. Something actually playing wins.
pub fn rank_players(mut candidates: Vec<PlayerState>) -> Option<PlayerState> {
    fn score(p: &PlayerState) -> u8 {
        match (p.playing, p.has_track) {
            (true, true) => 3,
            (false, true) => 2,
            (true, false) => 1,
            (false, false) => 0,
        }
    }
    // Name as the tiebreak, so the choice is stable between runs.
    candidates.sort_by(|a, b| score(b).cmp(&score(a)).then_with(|| a.name.cmp(&b.name)));
    candidates.into_iter().next()
}

/// Ask every player on the bus what it is doing.
pub async fn survey(conn: &Connection) -> Result<Vec<PlayerState>> {
    let names = list_players(conn).await?;
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let (playing, has_track) = match MprisPlayerHandle::connect(conn, &name).await {
            Ok(h) => (h.playing().await, h.track().await.is_some_and(|t| t.is_usable())),
            Err(_) => (false, false),
        };
        out.push(PlayerState {
            name,
            playing,
            has_track,
        });
    }
    Ok(out)
}

/// Pick the player to follow. `wanted` matches case-insensitively on either the
/// short name (`spotify`) or the full bus name.
pub async fn resolve_player(conn: &Connection, wanted: Option<&str>) -> Result<String> {
    let players = list_players(conn).await?;
    if players.is_empty() {
        return Err(anyhow!(
            "no MPRIS player found on the session bus — start a player and try again"
        ));
    }
    match wanted {
        None => {
            let ranked = rank_players(survey(conn).await?);
            Ok(ranked.map(|p| p.name).unwrap_or_else(|| players[0].clone()))
        }
        Some(w) => {
            let w = w.trim().trim_start_matches(MPRIS_PREFIX);
            players
                .iter()
                .find(|p| p.eq_ignore_ascii_case(w))
                .or_else(|| {
                    players
                        .iter()
                        .find(|p| p.to_lowercase().starts_with(&w.to_lowercase()))
                })
                .cloned()
                .ok_or_else(|| {
                    anyhow!(
                        "no MPRIS player matching {w:?}; available: {}",
                        players.join(", ")
                    )
                })
        }
    }
}

pub struct MprisPlayerHandle {
    proxy: MprisPlayerProxy<'static>,
    pub name: String,
}

impl MprisPlayerHandle {
    pub async fn connect(conn: &Connection, name: &str) -> Result<Self> {
        let bus_name = format!("{MPRIS_PREFIX}{name}");
        let proxy = MprisPlayerProxy::builder(conn)
            .destination(bus_name.clone())?
            .path(MPRIS_PATH)?
            .build()
            .await
            .with_context(|| format!("failed to connect to {bus_name}"))?;
        Ok(Self {
            proxy,
            name: name.to_string(),
        })
    }

    pub async fn track(&self) -> Option<Track> {
        let md = self.proxy.metadata().await.ok()?;
        track_from_metadata(&md)
    }

    pub async fn position(&self) -> Option<f64> {
        self.proxy.position().await.ok().map(us_to_secs)
    }

    pub async fn playing(&self) -> bool {
        self.proxy
            .playback_status()
            .await
            .map(|s| status_is_playing(&s))
            .unwrap_or(false)
    }

    pub async fn rate(&self) -> f64 {
        match self.proxy.rate().await {
            Ok(r) if r.is_finite() && r > 0.0 => r,
            _ => 1.0,
        }
    }

    pub async fn play_pause(&self) -> Result<()> {
        self.proxy.play_pause().await?;
        Ok(())
    }

    /// Spawn the event pump: property changes, `Seeked`, and a slow `Position`
    /// poll. The poll is not belt-and-braces — Spotify does not emit `Seeked`,
    /// so without it a scrub would leave the lyrics stranded.
    pub fn spawn(self, poll_interval: Duration) -> (EventRx, tokio::task::JoinHandle<()>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = tokio::spawn(async move {
            if let Err(e) = self.pump(tx.clone(), poll_interval).await {
                let _ = tx.send(PlayerEvent::Gone);
                tracing_note(&format!("player event stream ended: {e:#}"));
            }
        });
        (rx, handle)
    }

    async fn pump(self, tx: EventTx, poll_interval: Duration) -> Result<()> {
        let mut metadata_changes = self.proxy.receive_metadata_changed().await;
        let mut status_changes = self.proxy.receive_playback_status_changed().await;
        let mut seeks = self.proxy.receive_seeked().await?;
        let mut ticker = tokio::time::interval(poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // Prime the consumer with the current state before any event arrives.
        let track = self.track().await;
        let _ = tx.send(PlayerEvent::Track(track.map(Box::new)));
        let _ = tx.send(PlayerEvent::Status {
            playing: self.playing().await,
            position: self.position().await.unwrap_or(0.0),
        });

        use futures_lite::StreamExt as _;
        loop {
            tokio::select! {
                Some(change) = metadata_changes.next() => {
                    let md = change.get().await.unwrap_or_default();
                    let track = track_from_metadata(&md);
                    if tx.send(PlayerEvent::Track(track.map(Box::new))).is_err() {
                        return Ok(());
                    }
                    // A new track resets the position; ask rather than assume.
                    let _ = tx.send(PlayerEvent::Status {
                        playing: self.playing().await,
                        position: self.position().await.unwrap_or(0.0),
                    });
                }
                Some(change) = status_changes.next() => {
                    let playing = change.get().await.map(|s| status_is_playing(&s)).unwrap_or(false);
                    if tx.send(PlayerEvent::Status {
                        playing,
                        position: self.position().await.unwrap_or(0.0),
                    }).is_err() {
                        return Ok(());
                    }
                }
                Some(seek) = seeks.next() => {
                    if let Ok(args) = seek.args()
                        && tx.send(PlayerEvent::Seeked { position: us_to_secs(args.position) }).is_err() {
                        return Ok(());
                    }
                }
                _ = ticker.tick() => {
                    match self.position().await {
                        Some(position) => {
                            if tx.send(PlayerEvent::Tick { position, playing: self.playing().await }).is_err() {
                                return Ok(());
                            }
                        }
                        None => {
                            // The player went away mid-poll.
                            let _ = tx.send(PlayerEvent::Gone);
                            return Ok(());
                        }
                    }
                }
                else => return Ok(()),
            }
        }
    }
}

/// Diagnostics go to stderr only when explicitly asked for; the TUI owns stdout.
fn tracing_note(msg: &str) {
    if std::env::var_os("LYRICS_DEBUG").is_some() {
        eprintln!("[lyrics] {msg}");
    }
}
