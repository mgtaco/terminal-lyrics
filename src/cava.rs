//! The spectrum strip, from a `cava` started from the user's own config, so
//! their input setup is kept and only the output side is replaced.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::watch;

/// Fixed rather than the strip's height, which changes from line to line; the
/// bars are scaled to the strip as they are drawn.
pub const MAX_LEVEL: u16 = 1000;

/// Sections of the user's config that are about the sound, and so are kept.
const KEPT_SECTIONS: &[&str] = &["general", "input", "smoothing", "eq"];

/// Keys in a kept section that are ours to decide, because they are about the
/// shape of what is drawn rather than the sound.
const OURS: &[&str] = &["bars", "bar_width", "bar_spacing", "framerate"];

/// Keys of the user's `[output]` that still apply to raw output.
const KEPT_OUTPUT: &[&str] = &["channels", "mono_option", "reverse"];

/// The user's config is filtered line by line rather than parsed, so comments
/// and unknown keys pass through unchanged.
pub fn config_text(user: Option<&str>, bars: usize, framerate: u32) -> String {
    let mut kept: Vec<(String, Vec<String>)> = Vec::new();
    let mut output: Vec<String> = Vec::new();
    let mut section = String::new();

    for line in user.unwrap_or_default().lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with(';') || t.starts_with('#') {
            continue;
        }
        if let Some(name) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            section = name.trim().to_ascii_lowercase();
            continue;
        }
        let key = t
            .split('=')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if section == "output" {
            if KEPT_OUTPUT.contains(&key.as_str()) {
                output.push(t.to_string());
            }
            continue;
        }
        if !KEPT_SECTIONS.contains(&section.as_str()) || OURS.contains(&key.as_str()) {
            continue;
        }
        match kept.iter_mut().find(|(s, _)| *s == section) {
            Some((_, lines)) => lines.push(t.to_string()),
            None => kept.push((section.clone(), vec![t.to_string()])),
        }
    }

    let mut out = String::from("[general]\n");
    out += &format!("bars = {bars}\nframerate = {framerate}\n");
    for (name, lines) in &kept {
        if name != "general" {
            out += &format!("\n[{name}]\n");
        }
        for l in lines {
            out += l;
            out += "\n";
        }
    }
    out += "\n[output]\nmethod = raw\nraw_target = /dev/stdout\ndata_format = ascii\n";
    out += &format!("ascii_max_range = {MAX_LEVEL}\n");
    // `;` between bars, a newline after each frame.
    out += "bar_delimiter = 59\nframe_delimiter = 10\n";
    for l in &output {
        out += l;
        out += "\n";
    }
    out
}

/// `3;7;0;12;`: the trailing `;` is cava's, not an empty bar. Garbage reads as
/// silence, since a garbled frame is gone a frame later.
pub fn parse_frame(line: &str) -> Vec<u16> {
    line.trim()
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().parse().unwrap_or(0))
        .collect()
}

/// `$XDG_CONFIG_HOME/cava/config`, which is where cava itself looks on every
/// platform — not the platform config dir, which on macOS is elsewhere.
fn user_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().join(".config")))?;
    Some(base.join("cava").join("config"))
}

/// A running cava, and the latest frame it has sent.
pub struct Cava {
    /// How many bars it was started with. A running cava is fixed to this; a
    /// different count means a new cava.
    pub bars: usize,
    frames: watch::Receiver<Vec<u16>>,
    /// Held so the process dies with this value.
    _child: Child,
    config: PathBuf,
}

/// Each cava gets its own config file, so a replacement starting up never
/// reads one the old one's cleanup is deleting.
static SPAWNED: AtomicUsize = AtomicUsize::new(0);

impl Cava {
    /// Start cava with `bars` bars. `NotFound` means cava is not installed.
    pub fn spawn(bars: usize, framerate: u32) -> std::io::Result<Self> {
        let user = user_config_path().and_then(|p| std::fs::read_to_string(p).ok());
        let n = SPAWNED.fetch_add(1, Ordering::Relaxed);
        let config = std::env::temp_dir().join(format!(
            "terminal-lyrics-cava-{}-{n}.conf",
            std::process::id()
        ));
        std::fs::write(&config, config_text(user.as_deref(), bars, framerate))?;

        let child = Command::new("cava")
            .arg("-p")
            .arg(&config)
            // A PipeWire `source` that is not there (the player is closed) would
            // otherwise fall back to the default input: the mic. Wait for it.
            .env(
                "PIPEWIRE_PROPS",
                "{ node.dont-fallback = true node.linger = true }",
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_file(&config);
                return Err(e);
            }
        };

        let stdout = child.stdout.take().expect("stdout was piped");
        let (tx, frames) = watch::channel(Vec::new());
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let frame = parse_frame(&line);
                // Only a frame that differs wakes the loop, so silence — the
                // same row of zeros over and over — costs no redraws.
                tx.send_if_modified(|cur| {
                    let changed = *cur != frame;
                    if changed {
                        *cur = frame;
                    }
                    changed
                });
            }
        });

        Ok(Self {
            bars,
            frames,
            _child: child,
            config,
        })
    }

    pub fn levels(&self) -> Vec<u16> {
        self.frames.borrow().clone()
    }

    /// Resolves when a new frame arrives. A cava that has exited never sends
    /// another, and this then never resolves.
    pub async fn changed(&mut self) {
        if self.frames.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

impl Drop for Cava {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.config);
    }
}
