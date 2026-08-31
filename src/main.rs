//! `lyrics` — giant block lyrics in the terminal, synced over MPRIS.

use std::time::Duration;

use anyhow::Result;
use clap::Parser;

use terminal_lyrics::cli::{Cli, Command};
use terminal_lyrics::config::{self, ColorSource, Config, ConfigFile};
use terminal_lyrics::lyrics::cache::Cache;
use terminal_lyrics::lyrics::{self, Net, Outcome, Source};
use terminal_lyrics::offsets::Offsets;
use terminal_lyrics::player::{Session, Track};
use terminal_lyrics::tui;

/// How often to read `Position` from the player. This is the safety net for
/// players that do not emit `Seeked` — Spotify among them — not the main
/// source of truth, so once a second is plenty.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = cli.config.clone().or_else(config::default_config_path);
    let file = match &config_path {
        Some(p) => ConfigFile::load(p)?,
        None => ConfigFile::default(),
    };
    let cfg = Config::resolve(file, &cli);

    match cli.command.clone() {
        Some(Command::Paths) => {
            let show = |p: Option<std::path::PathBuf>| {
                p.map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(unavailable)".to_string())
            };
            println!("config  {}", show(config_path));
            println!("cache   {}", show(config::cache_dir()));
            let offsets = Offsets::new(config::offsets_path());
            println!(
                "offsets {}{}",
                show(config::offsets_path()),
                match offsets.len() {
                    0 => String::new(),
                    1 => "  (1 song tuned)".to_string(),
                    n => format!("  ({n} songs tuned)"),
                }
            );
            Ok(())
        }
        Some(Command::Fetch {
            artist,
            title,
            album,
            duration,
            spotify_id,
        }) => {
            cmd_fetch(
                &cfg,
                &artist,
                &title,
                album.as_deref(),
                duration,
                spotify_id.as_deref(),
            )
            .await
        }
        Some(Command::Status) => cmd_status(&cfg).await,
        None => cmd_run(cfg).await,
    }
}

fn client_for(cfg: &Config) -> Result<Option<Net>> {
    if cfg.network {
        Ok(Some(Net::new(
            cfg.lyricsplus_url.clone(),
            cfg.lrcmux_url.clone(),
        )?))
    } else {
        Ok(None)
    }
}

/// Headless lookup, so the fetch path can be exercised without a player.
///
/// This runs the same provider chain the live path does, on a `Track` built
/// from the flags — one chain, so a bug found here is the bug the visualiser
/// has too.
async fn cmd_fetch(
    cfg: &Config,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration: Option<f64>,
    spotify_id: Option<&str>,
) -> Result<()> {
    let Some(client) = client_for(cfg)? else {
        anyhow::bail!("--no-network is set, so there is nothing to fetch from");
    };

    let track = Track {
        // AMLL is keyed by this, so passing `--spotify-id` is what puts it in
        // play; without one the chain simply skips it.
        id: spotify_id.unwrap_or_default().to_string(),
        title: title.to_string(),
        artist: artist.to_string(),
        album: album.map(str::to_string),
        length: duration,
    };

    match lyrics::first_hit(&client, &cfg.providers, &track).await? {
        Some(found) => {
            eprintln!(
                "# {} · {} · {} lines",
                found.source,
                if found.synced { "synced" } else { "unsynced" },
                found.lyrics.lines.len()
            );
            print!("{}", found.raw);
            if !found.raw.ends_with('\n') {
                println!();
            }
            Ok(())
        }
        None => {
            eprintln!("no lyrics found for {artist} — {title}");
            std::process::exit(1);
        }
    }
}

/// Everything the visualiser would resolve, printed as plain lines.
async fn cmd_status(cfg: &Config) -> Result<()> {
    let session = Session::open().await?;

    // Show what each player is doing, so "it followed the wrong one" is
    // answerable at a glance.
    let survey = session.survey().await?;
    if survey.is_empty() {
        println!("players   (none)");
    } else {
        let described: Vec<String> = survey
            .iter()
            .map(|p| {
                let what = match (p.playing, p.has_track) {
                    (true, true) => "playing",
                    (false, true) => "paused",
                    (_, false) => "no track",
                };
                format!("{} ({what})", p.name)
            })
            .collect();
        println!("players   {}", described.join(", "));
    }

    let name = session.resolve(cfg.player.as_deref()).await?;
    println!("following {name}");
    println!(
        "offset    {}ms{}  — where a song starts before you nudge it",
        cfg.offset_ms,
        if cfg.offset_ms == 0 {
            ""
        } else if cfg.offset_ms < 0 {
            "  (lyrics shown earlier)"
        } else {
            "  (lyrics shown later)"
        }
    );

    let handle = session.connect(&name).await?;
    let playing = handle.playing().await;
    let position = handle.position().await;
    let track = handle.track();
    let track = track.await;

    println!("status    {}", if playing { "playing" } else { "paused" });
    println!(
        "position  {}",
        position
            .map(|p| format!("{p:.2}s"))
            .unwrap_or_else(|| "(unavailable)".into())
    );

    let Some(track) = track else {
        println!("track     (none)");
        return Ok(());
    };

    println!("track     {}", track.label());
    println!("album     {}", track.album.as_deref().unwrap_or("(none)"));
    println!(
        "length    {}",
        track
            .length
            .map(|d| format!("{d:.2}s"))
            .unwrap_or_else(|| "(not reported)".into())
    );
    println!("cache key {}", track.id);
    match Offsets::new(config::offsets_path()).get(&track.id) {
        Some(ms) => println!("tuned     {ms:+}ms  (saved for this song)"),
        None => println!("tuned     no  (using the default above)"),
    }
    println!(
        "providers {}",
        if cfg.providers.is_empty() {
            "(none)".to_string()
        } else {
            cfg.providers
                .iter()
                .map(|p| p.name())
                .collect::<Vec<_>>()
                .join(" -> ")
        }
    );
    println!(
        "colour    {}{}",
        cfg.color_source.label(),
        // A source that resolves to nothing has fallen back to the terminal
        // palette — pywal never ran, or the path is wrong. Worth saying, since
        // the alternative is wondering why the colour did not change.
        match (&cfg.color_source, cfg.color_source.accent()) {
            (ColorSource::Terminal, _) => String::new(),
            (_, Some([r, g, b])) => format!("  (accent #{r:02x}{g:02x}{b:02x})"),
            (_, None) => "  (unreadable — using the terminal palette)".to_string(),
        }
    );

    let cache = Cache::new(config::cache_dir());
    if let Some(dir) = cfg.lrc_dir.as_deref()
        && let Some(found) = terminal_lyrics::lyrics::local_lookup(dir, &track)
    {
        report_lyrics(cfg, &found);
        return Ok(());
    }

    let Some(client) = client_for(cfg)? else {
        println!("lyrics    (network disabled)");
        return Ok(());
    };

    match lyrics::lookup(&client, &cfg.providers, &cache, &track).await? {
        Outcome::Found(found) => {
            report_lyrics(cfg, &found);
            if matches!(found.source, Source::LrcLib { .. }) {
                println!("          (now cached)");
            }
        }
        Outcome::Missing => println!("lyrics    (none found)"),
    }
    Ok(())
}

/// One place that describes a set of lyrics, so every path through `status`
/// reports the same fields.
fn report_lyrics(cfg: &Config, found: &terminal_lyrics::lyrics::Found) {
    let word_timed = found.lyrics.has_word_timings();
    println!(
        "lyrics    {} · {} · {} lines · {}",
        found.source,
        if found.synced { "synced" } else { "unsynced" },
        found.lyrics.lines.len(),
        if word_timed {
            "real word timings"
        } else {
            "line-level only"
        }
    );
    println!(
        "display   {}",
        if word_timed && cfg.word_by_word {
            "one word at a time"
        } else {
            "whole lines"
        }
    );
    println!(
        "highlight {} -> {}",
        cfg.sweep.label(),
        if cfg.sweep.applies(word_timed, cfg.word_by_word) {
            "on"
        } else {
            "off"
        }
    );
}

async fn cmd_run(cfg: Config) -> Result<()> {
    let session = Session::open().await?;
    let name = session.resolve(cfg.player.as_deref()).await?;
    let handle = session.connect(&name).await?;

    // The pump needs its own handle; the UI keeps one for play/pause.
    let control = session.connect(&name).await?;
    let (events, pump) = handle.spawn(POLL_INTERVAL);

    let cache = Cache::new(config::cache_dir());
    let client = client_for(&cfg)?;
    let offsets = Offsets::new(config::offsets_path());

    let result = tui::run(cfg, control, events, cache, client, offsets).await;
    pump.abort();
    result
}
