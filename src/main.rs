//! `lyrics` — giant block lyrics in the terminal, synced over MPRIS.

use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use zbus::Connection;

use terminal_lyrics::cli::{Cli, Command};
use terminal_lyrics::config::{self, Config, ConfigFile};
use terminal_lyrics::lyrics::cache::Cache;
use terminal_lyrics::lyrics::lrclib::{self, LrcLib};
use terminal_lyrics::lyrics::{Outcome, Source};
use terminal_lyrics::player::mpris::{self, MprisPlayerHandle};
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
            Ok(())
        }
        Some(Command::Fetch {
            artist,
            title,
            album,
            duration,
        }) => cmd_fetch(&cfg, &artist, &title, album.as_deref(), duration).await,
        Some(Command::Status) => cmd_status(&cfg).await,
        None => cmd_run(cfg).await,
    }
}

fn client_for(cfg: &Config) -> Result<Option<LrcLib>> {
    if cfg.network {
        Ok(Some(LrcLib::new()?))
    } else {
        Ok(None)
    }
}

/// Headless lookup, so the fetch path can be exercised without a player.
async fn cmd_fetch(
    cfg: &Config,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration: Option<f64>,
) -> Result<()> {
    let Some(client) = client_for(cfg)? else {
        anyhow::bail!("--no-network is set, so there is nothing to fetch from");
    };
    match lrclib::fetch(&client, artist, title, album, duration).await? {
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
    let conn = Connection::session()
        .await
        .context("could not reach the session bus")?;

    let players = mpris::list_players(&conn).await?;
    println!(
        "players   {}",
        if players.is_empty() {
            "(none)".to_string()
        } else {
            players.join(", ")
        }
    );

    let name = mpris::resolve_player(&conn, cfg.player.as_deref()).await?;
    println!("following {name}");

    let handle = MprisPlayerHandle::connect(&conn, &name).await?;
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

    let cache = Cache::new(config::cache_dir());
    if let Some(dir) = cfg.lrc_dir.as_deref()
        && let Some(found) = terminal_lyrics::lyrics::local_lookup(dir, &track)
    {
        println!("lyrics    {} ({} lines)", found.source, found.lyrics.lines.len());
        return Ok(());
    }

    let Some(client) = client_for(cfg)? else {
        println!("lyrics    (network disabled)");
        return Ok(());
    };

    match lrclib::lookup(&client, &cache, &track).await? {
        Outcome::Found(found) => {
            println!(
                "lyrics    {} · {} · {} lines · {}",
                found.source,
                if found.synced { "synced" } else { "unsynced" },
                found.lyrics.lines.len(),
                if found.lyrics.has_word_timings() {
                    "real word timings"
                } else {
                    "interpolated sweep"
                }
            );
            if matches!(found.source, Source::LrcLib { .. }) {
                println!("          (now cached)");
            }
        }
        Outcome::Missing => println!("lyrics    (none found)"),
    }
    Ok(())
}

async fn cmd_run(cfg: Config) -> Result<()> {
    let conn = Connection::session()
        .await
        .context("could not reach the session bus — is this a desktop session?")?;
    let name = mpris::resolve_player(&conn, cfg.player.as_deref()).await?;
    let handle = MprisPlayerHandle::connect(&conn, &name).await?;

    // The pump needs its own handle; the UI keeps one for play/pause.
    let control = MprisPlayerHandle::connect(&conn, &name).await?;
    let (events, pump) = handle.spawn(POLL_INTERVAL);

    let cache = Cache::new(config::cache_dir());
    let client = client_for(&cfg)?;

    let result = tui::run(cfg, control, events, cache, client).await;
    pump.abort();
    result
}
