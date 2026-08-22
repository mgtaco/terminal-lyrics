//! Terminal lifecycle and the event loop.
//!
//! The loop is woken by whichever comes first: a key, a player event, a lyrics
//! lookup finishing, or the redraw tick. Only the tick is periodic, and all it
//! does is advance the sweep — ratatui diffs the buffer, so an unchanged screen
//! costs no output at all. v1 wrote a full clear plus a full screen every 50ms.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures_lite::StreamExt as _;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::Paragraph;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::lyrics::cache::Cache;
use crate::lyrics::lrclib::{self, LrcLib};
use crate::lyrics::{Outcome, Source};
use crate::player::mpris::MprisPlayerHandle;
use crate::player::{EventRx, PlayerEvent, Track};
use crate::render::font::{self, Font};
use crate::render::{Screen, Theme};
use crate::sync::{Change, SyncEngine};
use crate::timeline::{Position, Timeline};

/// What the lyric area is showing.
enum LyricState {
    /// No track at all.
    Idle,
    /// A track is loaded and a lookup is in flight.
    Searching,
    /// Lyrics are loaded.
    Ready {
        timeline: Box<Timeline>,
        source: Source,
        synced: bool,
    },
    /// Looked up, nothing usable found.
    Missing,
}

/// A finished background lookup, tagged with the track it was for so a slow
/// result for a previous song cannot overwrite the current one.
struct FetchResult {
    track_id: String,
    outcome: Result<Outcome>,
}

pub struct App {
    cfg: Config,
    engine: SyncEngine,
    state: LyricState,
    font: Font,
    theme: Theme,
    /// Sticky message shown briefly after a keypress, e.g. the new offset.
    notice: Option<(String, Instant)>,
    should_quit: bool,
}

impl App {
    pub fn new(cfg: Config, now: Instant) -> Self {
        let font = font::by_name(&cfg.font).unwrap_or_else(font::block);
        let engine = SyncEngine::new(
            cfg.offset_ms,
            Duration::from_millis(cfg.resync_threshold_ms),
            now,
        );
        Self {
            cfg,
            engine,
            state: LyricState::Idle,
            font,
            theme: Theme::default(),
            notice: None,
            should_quit: false,
        }
    }

    fn note(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), Instant::now()));
    }

    fn notice_text(&self) -> Option<&str> {
        let (msg, at) = self.notice.as_ref()?;
        (at.elapsed() < Duration::from_millis(1500)).then_some(msg.as_str())
    }
}

/// Run the visualiser until the user quits or the player disappears.
pub async fn run(
    cfg: Config,
    player: MprisPlayerHandle,
    events: EventRx,
    cache: Cache,
    client: Option<LrcLib>,
) -> Result<()> {
    let terminal = ratatui::try_init().context("failed to take over the terminal")?;
    let result = run_inner(cfg, player, events, cache, client, terminal).await;
    ratatui::restore();
    result
}

async fn run_inner(
    cfg: Config,
    player: MprisPlayerHandle,
    mut events: EventRx,
    cache: Cache,
    client: Option<LrcLib>,
    mut terminal: DefaultTerminal,
) -> Result<()> {
    let start = Instant::now();
    let mut app = App::new(cfg, start);
    let mut keys = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(app.cfg.tick_ms));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let (fetch_tx, mut fetch_rx) = mpsc::unbounded_channel::<FetchResult>();
    let client = client.map(std::sync::Arc::new);
    let cache = std::sync::Arc::new(cache);

    let mut sigterm = signal_stream();

    loop {
        draw(&mut terminal, &app)?;
        if app.should_quit {
            return Ok(());
        }

        tokio::select! {
            // Player state.
            Some(event) = events.recv() => {
                let now = Instant::now();
                let gone = matches!(event, PlayerEvent::Gone);
                match app.engine.apply(event, now) {
                    Change::Track(track) => {
                        start_lookup(&mut app, track.map(|b| *b), &cache, &client, &fetch_tx);
                    }
                    Change::Gone => {
                        app.state = LyricState::Idle;
                        app.note("player closed");
                    }
                    Change::Resynced | Change::None => {}
                }
                if gone {
                    return Ok(());
                }
            }

            // A background lookup finished.
            Some(result) = fetch_rx.recv() => {
                apply_fetch(&mut app, result);
            }

            // Keyboard.
            Some(Ok(event)) = keys.next() => {
                if let Event::Key(key) = event {
                    handle_key(&mut app, key, &player, &cache, &client, &fetch_tx).await;
                }
            }

            _ = ticker.tick() => {}

            _ = &mut sigterm => {
                return Ok(());
            }
        }
    }
}

/// SIGTERM as a future, so a `kill` restores the terminal like `q` does.
fn signal_stream() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            if let Ok(mut s) = signal(SignalKind::terminate()) {
                s.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        }
        #[cfg(not(unix))]
        std::future::pending::<()>().await;
    })
}

fn draw(terminal: &mut DefaultTerminal, app: &App) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        let now = Instant::now();
        let pos = app.engine.lyric_position(now);

        let label = app
            .engine
            .track()
            .map(Track::label)
            .unwrap_or_default();

        // Held so the borrow lives as long as `screen`.
        let mut line_text = String::new();
        let mut highlight = 0usize;

        let screen = match &app.state {
            LyricState::Idle => Screen::Idle {
                message: "nothing playing",
            },
            LyricState::Searching => Screen::Searching { label: &label },
            LyricState::Missing => Screen::NoLyrics { label: &label },
            LyricState::Ready { timeline, .. } => match timeline.locate(pos) {
                Position::Line { index } => {
                    if let Some(line) = timeline.line(index) {
                        line_text = line.text.clone();
                        highlight = if app.cfg.sweep {
                            timeline.highlight_chars(index, pos)
                        } else {
                            0
                        };
                    }
                    Screen::Lyric {
                        text: &line_text,
                        highlight,
                    }
                }
                // During the intro and the outro, show whose song it is.
                Position::Intro { .. } | Position::Outro => Screen::Idle { message: &label },
            },
        };

        let text = crate::render::render(
            &screen,
            &app.font,
            area.width,
            area.height.saturating_sub(1),
            app.theme,
        );
        frame.render_widget(Paragraph::new(text), area);

        // One-line status strip along the bottom.
        if let Some(status) = status_line(app)
            && area.height > 0
        {
            let strip = ratatui::layout::Rect {
                x: area.x,
                y: area.y + area.height - 1,
                width: area.width,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(status).style(ratatui::style::Style::default().fg(app.theme.dim)),
                strip,
            );
        }
    })?;
    Ok(())
}

fn status_line(app: &App) -> Option<String> {
    if let Some(notice) = app.notice_text() {
        return Some(format!(" {notice}"));
    }
    match &app.state {
        LyricState::Ready { source, synced, .. } => {
            let mut parts = vec![source.to_string()];
            if !*synced {
                parts.push("unsynced".into());
            }
            let off = app.engine.clock().offset_ms();
            if off != 0 {
                parts.push(format!("offset {off:+}ms"));
            }
            if !app.engine.is_playing() {
                parts.push("paused".into());
            }
            Some(format!(" {}", parts.join(" · ")))
        }
        _ => None,
    }
}

/// Kick off a lookup for a newly loaded track.
fn start_lookup(
    app: &mut App,
    track: Option<Track>,
    cache: &std::sync::Arc<Cache>,
    client: &Option<std::sync::Arc<LrcLib>>,
    tx: &mpsc::UnboundedSender<FetchResult>,
) {
    let Some(track) = track.filter(Track::is_usable) else {
        app.state = LyricState::Idle;
        return;
    };

    // A local file beats everything and needs no task.
    if let Some(dir) = app.cfg.lrc_dir.clone()
        && let Some(found) = crate::lyrics::local_lookup(&dir, &track)
    {
        app.state = LyricState::Ready {
            timeline: Box::new(Timeline::new(found.lyrics)),
            source: found.source,
            synced: found.synced,
        };
        return;
    }

    app.state = LyricState::Searching;

    // Cached answers are cheap; take them on this task so the UI never shows a
    // pointless "searching" flash for a song we already know about.
    if let Some(cached) = cache.get(&track.id) {
        match cached {
            Some(found) => {
                app.state = LyricState::Ready {
                    timeline: Box::new(Timeline::new(found.lyrics)),
                    source: found.source,
                    synced: found.synced,
                };
            }
            None => app.state = LyricState::Missing,
        }
        return;
    }

    let Some(client) = client.clone() else {
        app.state = LyricState::Missing;
        return;
    };

    let cache = cache.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        let outcome = lrclib::lookup(&client, &cache, &track).await;
        let _ = tx.send(FetchResult {
            track_id: track.id.clone(),
            outcome,
        });
    });
}

fn apply_fetch(app: &mut App, result: FetchResult) {
    // Ignore an answer for a track we have already moved on from.
    let current = app.engine.track().map(|t| t.id.clone());
    if current.as_deref() != Some(result.track_id.as_str()) {
        return;
    }
    match result.outcome {
        Ok(Outcome::Found(found)) => {
            app.state = LyricState::Ready {
                timeline: Box::new(Timeline::new(found.lyrics)),
                source: found.source,
                synced: found.synced,
            };
        }
        Ok(Outcome::Missing) => app.state = LyricState::Missing,
        Err(e) => {
            app.state = LyricState::Missing;
            app.note(format!("lookup failed: {e}"));
        }
    }
}

async fn handle_key(
    app: &mut App,
    key: KeyEvent,
    player: &MprisPlayerHandle,
    cache: &std::sync::Arc<Cache>,
    client: &Option<std::sync::Arc<LrcLib>>,
    tx: &mpsc::UnboundedSender<FetchResult>,
) {
    if key.kind != KeyEventKind::Press {
        return;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Char(',') => {
            app.engine.clock_mut().nudge_offset_ms(-100);
            let off = app.engine.clock().offset_ms();
            app.note(format!("offset {off:+}ms"));
        }
        KeyCode::Char('.') => {
            app.engine.clock_mut().nudge_offset_ms(100);
            let off = app.engine.clock().offset_ms();
            app.note(format!("offset {off:+}ms"));
        }
        KeyCode::Char('0') => {
            app.engine.clock_mut().set_offset_ms(0);
            app.note("offset reset");
        }
        KeyCode::Char('f') => {
            let next = font::next_after(app.font.name);
            app.font = font::by_name(next).unwrap_or_else(font::block);
            app.cfg.font = next.to_string();
            app.note(format!("font: {next}"));
        }
        KeyCode::Char('s') => {
            app.cfg.sweep = !app.cfg.sweep;
            app.note(if app.cfg.sweep {
                "sweep on"
            } else {
                "sweep off"
            });
        }
        KeyCode::Char('r') => {
            if let Some(track) = app.engine.track().cloned() {
                cache.forget(&track.id);
                app.note("refetching");
                start_lookup(app, Some(track), cache, client, tx);
            }
        }
        KeyCode::Char(' ') => {
            // The D-Bus connection is already open, so this costs nothing extra.
            if let Err(e) = player.play_pause().await {
                app.note(format!("play/pause failed: {e}"));
            }
        }
        _ => {}
    }
}
