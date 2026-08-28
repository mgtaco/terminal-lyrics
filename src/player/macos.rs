//! macOS, via AppleScript. One `osascript` per poll.
//!
//! There is no MPRIS here and no general now-playing API left to use: the
//! MediaRemote private framework that every other tool reached for
//! (`MRMediaRemoteGetNowPlayingInfo`) was restricted to entitled applications in
//! macOS 15.4, so it is dead on anything current. What remains is each app's own
//! scripting dictionary, which is why this backend knows the apps by name rather
//! than discovering them.
//!
//! That trade is not all loss. Spotify's dictionary hands back
//! `spotify:track:<id>`, which is exactly the AMLL lookup key, so the
//! word-by-word path — the whole point of the program — works here as well as it
//! does on Linux.
//!
//! Two decisions in the script below are load-bearing:
//!
//! * Every app is guarded by `is running`. A bare `tell application "Spotify"`
//!   *launches* Spotify, and a lyrics visualiser that opens your music player
//!   because you ran it is not acceptable behaviour.
//! * Every number crosses the boundary as an integer of milliseconds.
//!   AppleScript's `as text` for reals uses the decimal separator of the user's
//!   locale, so `6.55` arrives as `6,55` on a German Mac and parses as garbage.
//!   Integers have no separator.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use tokio::sync::mpsc;

use super::{EventRx, EventTx, PlayerEvent, PlayerState, Snapshot, Track, fallback_id};

/// How long to wait for `osascript` before giving up on a poll. A wedged player
/// should cost one skipped tick, not a frozen visualiser.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// How many probes in a row must fail before the player counts as gone. One
/// failed subprocess is a hiccup; three seconds of them is a departure.
const MISSES_BEFORE_GONE: u32 = 3;

/// AppleScript's "not authorised to send Apple events" error. This is the one
/// every new user hits, so it is the one error worth naming.
const ERR_NOT_AUTHORISED: i32 = -1743;

/// A scriptable player we know how to talk to.
struct App {
    /// The name the user types after `--player`, matching the Linux short names.
    name: &'static str,
    bundle: &'static str,
    /// AppleScript for the track's stable id, which differs per app. It runs
    /// where `lyTrack` is bound to `current track`.
    id_expr: &'static str,
    /// What to multiply `duration` by to get milliseconds.
    ///
    /// Music reports seconds, as its dictionary says. Spotify's dictionary also
    /// says "The length of the track in seconds" and then returns `233000` for
    /// a track that is 233 seconds long — the same brand of lie as the metadata
    /// types in `mpris.rs`, and worth pinning down here, because getting it
    /// wrong does not fail: it just puts every LRCLIB candidate outside the
    /// five-second window and the track quietly gets no lyrics.
    duration_scale: &'static str,
}

const APPS: &[App] = &[
    App {
        name: "spotify",
        bundle: "com.spotify.client",
        id_expr: "id of lyTrack",
        duration_scale: "1",
    },
    App {
        name: "music",
        bundle: "com.apple.Music",
        id_expr: "persistent ID of lyTrack",
        duration_scale: "1000",
    },
];

fn app_by_name(name: &str) -> Option<&'static App> {
    APPS.iter().find(|a| a.name.eq_ignore_ascii_case(name))
}

/// Why an app could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeError {
    /// TCC refused. The user has to grant the terminal Automation access.
    NotAuthorised,
    /// Anything else: not installed, no scripting dictionary, app quitting.
    Unavailable,
}

/// One app's line of the probe output.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub name: String,
    pub result: Result<Snapshot, ProbeError>,
}

/// The script sent to `osascript`.
///
/// Every variable carries an `ly` prefix because AppleScript resolves bare
/// identifiers against the target application's scripting dictionary first: a
/// variable called `st` is a syntax error inside `tell application "Spotify"`,
/// since Spotify's terminology already claims it.
///
/// Each app is wrapped in its own `try` so that a machine without Spotify
/// installed reports Spotify as unavailable rather than failing the whole probe
/// and hiding Music behind it. Arithmetic happens outside the `tell` block:
/// commands inside one are addressed to the application first, and `round`
/// belongs to Standard Additions, not to Spotify.
fn probe_script() -> String {
    let mut s = String::from("set AppleScript's text item delimiters to tab\nset lyOut to {}\n");
    for app in APPS {
        s.push_str(&format!(
            r#"try
	if application id "{bundle}" is running then
		set lyPos to missing value
		set lyDur to missing value
		set lyId to ""
		set lyTitle to ""
		set lyArtist to ""
		set lyAlbum to ""
		tell application id "{bundle}"
			set lyState to player state as text
			try
				set lyTrack to current track
				set lyPos to player position
				set lyDur to duration of lyTrack
				set lyId to (({id_expr}) as text)
				set lyTitle to ((name of lyTrack) as text)
				set lyArtist to ((artist of lyTrack) as text)
				set lyAlbum to ((album of lyTrack) as text)
			end try
		end tell
		set end of lyOut to {{"{name}", lyState, lyMs(lyPos, 1000), lyMs(lyDur, {duration_scale}), lyId, lyTitle, lyArtist, lyAlbum}} as text
	end if
on error errMsg number errNum
	set end of lyOut to {{"{name}", "!error", (errNum as text), "0", "", "", "", ""}} as text
end try
"#,
            bundle = app.bundle,
            name = app.name,
            id_expr = app.id_expr,
            duration_scale = app.duration_scale,
        ));
    }
    s.push_str(
        "set AppleScript's text item delimiters to linefeed\nreturn lyOut as text\n\n\
         on lyMs(v, scale)\n\
         \ttry\n\
         \t\treturn ((round (v * scale)) as integer) as text\n\
         \ton error\n\
         \t\treturn \"0\"\n\
         \tend try\n\
         end lyMs\n",
    );
    s
}

fn parse_ms(field: &str) -> Option<f64> {
    field
        .trim()
        .parse::<i64>()
        .ok()
        .map(|ms| ms as f64 / 1000.0)
}

/// Turn the probe's output into one [`Probe`] per line.
///
/// Deliberately forgiving: a track title is arbitrary user text, so a line that
/// does not look like a row is skipped rather than trusted. The free-text fields
/// come last precisely so that a stray tab inside a title cannot shift the
/// position or duration out from under the parser.
pub fn parse_probe(stdout: &str) -> Vec<Probe> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.splitn(8, '\t').collect();
        if f.len() != 8 {
            continue;
        }
        let (name, state) = (f[0], f[1]);
        if app_by_name(name).is_none() {
            continue;
        }

        if state == "!error" {
            let err = match f[2].trim().parse::<i32>() {
                Ok(ERR_NOT_AUTHORISED) => ProbeError::NotAuthorised,
                _ => ProbeError::Unavailable,
            };
            out.push(Probe {
                name: name.to_string(),
                result: Err(err),
            });
            continue;
        }

        let title = f[5].trim().to_string();
        let artist = f[6].trim().to_string();
        let track = (!title.is_empty()).then(|| {
            let id = f[4].trim();
            Track {
                id: if id.is_empty() {
                    fallback_id(&artist, &title)
                } else {
                    id.to_string()
                },
                title: title.clone(),
                artist: artist.clone(),
                album: Some(f[7].trim().to_string()).filter(|s| !s.is_empty()),
                length: parse_ms(f[3]).filter(|&d| d > 0.0),
            }
        });

        out.push(Probe {
            name: name.to_string(),
            result: Ok(Snapshot {
                playing: state.eq_ignore_ascii_case("playing"),
                position: parse_ms(f[2]).unwrap_or(0.0).max(0.0),
                track,
            }),
        });
    }
    out
}

/// The error a denied Automation prompt deserves. The raw AppleScript text says
/// nothing about where to go and fixing it is three clicks deep in Settings.
fn not_authorised(app: &str) -> anyhow::Error {
    anyhow!(
        "not authorised to control {app} — grant your terminal access under \
         System Settings → Privacy & Security → Automation, then try again"
    )
}

/// Build an `osascript` invocation for `script`.
///
/// The `process_group` call is the whole reason this helper exists. A child
/// inherits its parent's process group, which — since the TUI is the terminal's
/// foreground job — makes every probe briefly the foreground job too. Terminal
/// and iTerm both title their window after that job, so at one subprocess per
/// poll the title flickers between "lyrics" and "osascript" for as long as the
/// program runs. Its own group keeps it out of that lookup entirely.
///
/// Nothing is lost by detaching: `output()` gives the child a null stdin and
/// pipes for the rest, so it neither reads nor writes the terminal, and a probe
/// short enough to finish inside [`PROBE_TIMEOUT`] does not need the terminal's
/// Ctrl-C — which raw mode has switched off anyway.
fn osascript(script: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("osascript");
    cmd.arg("-e").arg(script).process_group(0);
    cmd
}

async fn run_probe() -> Result<Vec<Probe>> {
    let script = probe_script();
    let run = osascript(&script).output();

    let output = tokio::time::timeout(PROBE_TIMEOUT, run)
        .await
        .map_err(|_| anyhow!("osascript did not answer within {PROBE_TIMEOUT:?}"))?
        .context("failed to run osascript — is this really macOS?")?;

    // Every expected failure is reported inside the output as an `!error` row,
    // so a non-zero exit means the script itself did not run.
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-1743") {
            return Err(not_authorised("your music player"));
        }
        return Err(anyhow!("osascript failed: {}", stderr.trim()));
    }
    Ok(parse_probe(&String::from_utf8_lossy(&output.stdout)))
}

/// No connection to hold open: each probe is its own subprocess.
pub struct Session;

impl Session {
    pub async fn open() -> Result<Self> {
        Ok(Self)
    }

    /// Ask every app we know about what it is doing.
    ///
    /// An app that is not installed simply does not appear. A denial is
    /// different: it is reported only when it leaves nothing usable behind, so
    /// that refusing Spotify does not stop Music from working.
    pub async fn survey(&self) -> Result<Vec<PlayerState>> {
        let probes = run_probe().await?;
        let mut out = Vec::new();
        let mut denied: Option<String> = None;
        for probe in probes {
            match probe.result {
                Ok(snap) => out.push(PlayerState {
                    name: probe.name,
                    playing: snap.playing,
                    has_track: snap.track.is_some_and(|t| t.is_usable()),
                }),
                Err(ProbeError::NotAuthorised) => denied = Some(probe.name),
                Err(ProbeError::Unavailable) => {}
            }
        }
        match denied {
            Some(app) if out.is_empty() => Err(not_authorised(&app)),
            _ => Ok(out),
        }
    }

    pub async fn resolve(&self, wanted: Option<&str>) -> Result<String> {
        super::choose(
            self.survey().await?,
            wanted,
            "supported player (Spotify or Music)",
        )
    }

    pub async fn connect(&self, name: &str) -> Result<PlayerHandle> {
        let app = app_by_name(name).ok_or_else(|| {
            let known: Vec<&str> = APPS.iter().map(|a| a.name).collect();
            anyhow!(
                "{name:?} is not scriptable here; known players: {}",
                known.join(", ")
            )
        })?;
        Ok(PlayerHandle { app })
    }
}

pub struct PlayerHandle {
    app: &'static App,
}

impl PlayerHandle {
    pub fn name(&self) -> &'static str {
        self.app.name
    }

    /// Everything in one subprocess. `None` means the app is no longer running.
    pub async fn snapshot(&self) -> Option<Snapshot> {
        let probes = run_probe().await.ok()?;
        probes
            .into_iter()
            .find(|p| p.name == self.app.name)?
            .result
            .ok()
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
        let script = format!("tell application id \"{}\" to playpause", self.app.bundle);
        let output = osascript(&script)
            .output()
            .await
            .context("failed to run osascript")?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-1743") {
            return Err(not_authorised(self.app.name));
        }
        Err(anyhow!("play/pause failed: {}", stderr.trim()))
    }

    /// Spawn the event pump.
    ///
    /// Polling only — a scripting dictionary has nothing to subscribe to. That
    /// is less of a loss than it sounds: the Linux backend already polls for
    /// exactly this reason, because Spotify never emits `Seeked` there either,
    /// so [`crate::sync::SyncEngine`]'s `Tick` arm was written to re-anchor on
    /// divergence and picks up a scrub here the same way.
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
            // `interval` yields immediately the first time, so the consumer is
            // primed on the same tick rather than a poll interval late.
            ticker.tick().await;

            let snap = match run_probe().await {
                // The probe ran and our app was not in it: it really has quit.
                Ok(rows) => match rows.into_iter().find(|p| p.name == self.app.name) {
                    Some(row) => match row.result {
                        Ok(snap) => snap,
                        // Permission revoked while running, or the app is in the
                        // middle of quitting. Either way, stop following it.
                        Err(_) => {
                            let _ = tx.send(PlayerEvent::Gone);
                            return;
                        }
                    },
                    None => {
                        let _ = tx.send(PlayerEvent::Gone);
                        return;
                    }
                },
                // The probe itself failed — a timeout, a busy machine. That is
                // not the same as the player being gone, and tearing the
                // visualiser down over one unlucky subprocess would be a poor
                // trade. Spawning a process is simply less reliable than reading
                // a bus, so this backend has to tolerate the occasional miss.
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
                // outright instead of waiting for the drift check to notice.
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
