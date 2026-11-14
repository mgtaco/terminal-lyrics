//! Only the redraw tick is periodic; ratatui diffs the buffer, so an unchanged
//! screen costs no output.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures_lite::StreamExt as _;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::widgets::Paragraph;
use tokio::sync::mpsc;

use crate::cava::{self, Cava};
use crate::config::Config;
use crate::lyrics::{Engine, Outcome, Source};
use crate::offsets::Offsets;
use crate::player::{Link, PlayerEvent, PlayerHandle, Session, Track, pick_reconnect};
use crate::prefs::Prefs;
use crate::render::font::{self, Font};
use crate::render::{Screen, SecondVoice, Theme, bars};
use crate::sync::{Change, SyncEngine};
use crate::timeline::{Position, Timeline};

/// What the lyric area is showing.
enum LyricState {
    /// No track at all.
    Idle,
    /// A track is loaded and a lookup is in flight.
    Searching,
    Ready {
        timeline: Box<Timeline>,
        source: Source,
        /// Real per-word timestamps, decided once when the lyrics load.
        word_timed: bool,
        /// What the timeline was built from, so an answer that changes
        /// nothing does not rebuild it.
        raw: String,
    },
    /// Looked up, nothing usable found.
    Missing,
}

fn ready_from(found: crate::lyrics::Found) -> LyricState {
    let word_timed = found.lyrics.has_word_timings();
    LyricState::Ready {
        timeline: Box::new(Timeline::new(found.lyrics)),
        source: found.source,
        word_timed,
        raw: found.raw,
    }
}

/// A finished background lookup, tagged with the track it was for so a slow
/// result for a previous song cannot overwrite the current one.
struct FetchResult {
    track_id: String,
    /// What was asked, so an answer for a description of the track that has
    /// since been corrected is not shown over the corrected one.
    key: String,
    outcome: Result<Outcome>,
}

pub struct App {
    cfg: Config,
    engine: SyncEngine,
    /// Per-track sync nudges, reloaded on every track change and written back
    /// on every keypress that moves one.
    offsets: Offsets,
    /// Where the display keys save their choices: the config file.
    prefs: Prefs,
    state: LyricState,
    font: Font,
    theme: Theme,
    /// Sticky message shown briefly after a keypress, e.g. the new offset.
    notice: Option<(String, Instant)>,
    /// Why cava could not be started, once it has failed. It is not tried
    /// again: a cava that is not installed will not be on the next frame.
    cava_error: Option<String>,
    should_quit: bool,
}

impl App {
    pub fn new(cfg: Config, offsets: Offsets, prefs: Prefs, now: Instant) -> Self {
        let font = font::by_name(&cfg.font).unwrap_or_else(font::block);
        // Read once: the draw loop should not touch the filesystem.
        let theme = Theme::with_accent(cfg.color_source.accent()).with_dim(cfg.dim_color);
        let engine = SyncEngine::new(
            cfg.offset_ms,
            Duration::from_millis(cfg.resync_threshold_ms),
            now,
        );
        Self {
            cfg,
            engine,
            offsets,
            prefs,
            state: LyricState::Idle,
            font,
            theme,
            notice: None,
            cava_error: None,
            should_quit: false,
        }
    }

    /// Adopts the new track's saved offset here, where it cannot be forgotten;
    /// the caller still starts the lookup.
    pub fn apply_player_event(&mut self, event: PlayerEvent, now: Instant) -> Change {
        let change = self.engine.apply(event, now);
        if matches!(change, Change::Track(_)) {
            self.adopt_track_offset();
        }
        change
    }

    /// Called on every track change, so a correction for one song does not
    /// follow you into the next.
    pub fn adopt_track_offset(&mut self) {
        let key = self
            .engine
            .track()
            .map(|t| t.id.clone())
            .unwrap_or_default();
        let offset = self.offsets.offset_for(&key, self.cfg.offset_ms);
        self.engine.clock_mut().set_offset_ms(offset);
    }

    /// Shift the current track and remember the result.
    pub fn nudge_offset(&mut self, delta_ms: i64) {
        self.engine.clock_mut().nudge_offset_ms(delta_ms);
        self.save_offset();
    }

    /// Back to the configured starting point, and forgotten.
    pub fn reset_offset(&mut self) {
        self.engine.clock_mut().set_offset_ms(self.cfg.offset_ms);
        if let Some(track) = self.engine.track() {
            let key = track.id.clone();
            self.offsets.clear(&key);
        }
    }

    /// An offset equal to the default drops the entry: absence spells "never
    /// tuned".
    fn save_offset(&mut self) {
        let Some(track) = self.engine.track() else {
            return;
        };
        let (key, label) = (track.id.clone(), track.label());
        let offset = self.engine.clock().offset_ms();
        if offset == self.cfg.offset_ms {
            self.offsets.clear(&key);
        } else {
            self.offsets.set(&key, &label, offset);
        }
    }

    /// The offset in force, for a test or a notice.
    pub fn offset_ms(&self) -> i64 {
        self.engine.clock().offset_ms()
    }

    /// A setting that cannot be saved still applies for this run, and the
    /// notice says why.
    fn remember(&mut self, key: &str, value: impl Into<toml_edit::Value>, msg: impl Into<String>) {
        let msg = msg.into();
        match self.prefs.set(key, value) {
            Ok(()) => self.note(msg),
            Err(e) => self.note(format!("{msg}  (not saved: {e:#})")),
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

/// How often to look for a player again once the one being followed closes.
const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);

/// A player that closes is not a reason to stop: the next one to start is
/// picked up.
pub async fn run(
    cfg: Config,
    session: Session,
    link: Link,
    lookup: Arc<Engine>,
    offsets: Offsets,
    prefs: Prefs,
) -> Result<()> {
    let terminal = ratatui::try_init().context("failed to take over the terminal")?;
    let result = run_inner(cfg, session, link, lookup, offsets, prefs, terminal).await;
    ratatui::restore();
    result
}

async fn run_inner(
    cfg: Config,
    session: Session,
    link: Link,
    lookup: Arc<Engine>,
    offsets: Offsets,
    prefs: Prefs,
    mut terminal: DefaultTerminal,
) -> Result<()> {
    let start = Instant::now();
    let mut app = App::new(cfg, offsets, prefs, start);
    let mut keys = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(app.cfg.tick_ms));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let (fetch_tx, mut fetch_rx) = mpsc::unbounded_channel::<FetchResult>();

    let mut sigterm = signal_stream();

    // `None` while no player is open.
    let mut link = Some(link);
    let mut retry = tokio::time::interval(RECONNECT_INTERVAL);
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // `None` while the bars are off, or do not fit.
    let mut cava: Option<Cava> = None;

    loop {
        sync_visualizer(&mut app, &mut cava, terminal.size()?);
        draw(&mut terminal, &app, cava.as_ref())?;
        if app.should_quit {
            return Ok(());
        }

        tokio::select! {
            event = next_event(&mut link) => {
                let now = Instant::now();
                let gone = matches!(event, PlayerEvent::Gone);
                match app.apply_player_event(event, now) {
                    Change::Track(track) => {
                        start_lookup(&mut app, track.map(|b| *b), &lookup, &fetch_tx);
                    }
                    Change::Gone => {
                        app.state = LyricState::Idle;
                        app.note("player closed");
                    }
                    Change::Resynced | Change::None => {}
                }
                if gone {
                    link = None;
                    retry.reset();
                }
            }

            // No player: see whether one has started.
            _ = retry.tick(), if link.is_none() => {
                let wanted = app.cfg.player.clone();
                let found = session
                    .survey()
                    .await
                    .ok()
                    .and_then(|states| pick_reconnect(states, wanted.as_deref()));
                if let Some(name) = found
                    && let Ok(opened) = Link::open(&session, &name).await
                {
                    app.note(format!("following {}", opened.name));
                    link = Some(opened);
                }
            }

            Some(result) = fetch_rx.recv() => {
                apply_fetch(&mut app, result);
            }

            Some(Ok(event)) = keys.next() => {
                if let Event::Key(key) = event {
                    let player = link.as_ref().map(|l| &l.control);
                    handle_key(&mut app, key, player, &lookup, &fetch_tx).await;
                }
            }

            _ = next_frame(&mut cava) => {}

            _ = ticker.tick() => {}

            _ = &mut sigterm => {
                return Ok(());
            }
        }
    }
}

/// The next event from the player being followed. A stream that ends without
/// saying so is a player gone all the same; with none open, this never fires.
async fn next_event(link: &mut Option<Link>) -> PlayerEvent {
    match link {
        Some(l) => l.events.recv().await.unwrap_or(PlayerEvent::Gone),
        None => std::future::pending().await,
    }
}

/// The next frame of bars; with no cava running, this never fires.
async fn next_frame(cava: &mut Option<Cava>) {
    match cava {
        Some(c) => c.changed().await,
        None => std::future::pending().await,
    }
}

/// How many bars fit the window as it is now, if they are wanted at all.
fn visualizer_bars(app: &App, size: ratatui::layout::Size) -> Option<usize> {
    if !app.cfg.visualizer || app.cava_error.is_some() {
        return None;
    }
    Some(bars::bar_count(usize::from(size.width))).filter(|&n| n > 0)
}

/// cava is told its bar count up front, so a resize that changes it means a new
/// cava. The height is worked out per frame, from the room the lyrics leave.
fn sync_visualizer(app: &mut App, cava: &mut Option<Cava>, size: ratatui::layout::Size) {
    let want = visualizer_bars(app, size);
    if cava.as_ref().map(|c| c.bars) == want {
        return;
    }
    // The old one goes first, so two are never capturing at once.
    *cava = None;
    let Some(bars) = want else {
        return;
    };
    // As often as the screen is redrawn, within what cava will do.
    let framerate = (1000 / app.cfg.tick_ms).clamp(10, 60) as u32;
    match Cava::spawn(bars, framerate) {
        Ok(c) => *cava = Some(c),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            app.cava_error = Some("cava is not installed".to_string());
        }
        Err(e) => {
            let msg = format!("cava failed to start: {e}");
            app.note(msg.clone());
            app.cava_error = Some(msg);
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

fn draw(terminal: &mut DefaultTerminal, app: &App, cava: Option<&Cava>) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        let now = Instant::now();
        let pos = app.engine.lyric_position(now);

        let label = app.engine.track().map(Track::label).unwrap_or_default();

        // Held so the borrow lives as long as `screen`.
        let mut line_text = String::new();
        let mut second_text = String::new();
        let mut second_background = false;
        let mut second_highlight = 0usize;
        let mut second_reveal = 0usize;
        let mut highlight = 0usize;
        let mut reveal = 0usize;

        let screen = match &app.state {
            LyricState::Idle => Screen::Idle {
                message: "nothing playing",
            },
            LyricState::Searching => Screen::Searching { label: &label },
            LyricState::Missing => Screen::NoLyrics { label: &label },
            LyricState::Ready {
                timeline,
                word_timed,
                ..
            } => match timeline.locate(pos) {
                Position::Line { index } => {
                    if let Some(line) = timeline.line(index) {
                        let sweep = app.cfg.sweep.applies(*word_timed, app.cfg.word_by_word);
                        highlight = if sweep {
                            timeline.highlight_chars(index, pos)
                        } else {
                            0
                        };

                        // The parser always works out the second voice; this
                        // only decides whether we ask.
                        let mut voice = None;
                        if app.cfg.overlapping_voices
                            && let Some(s) = timeline.secondary(index, pos)
                            && !s.text.trim().is_empty()
                        {
                            second_text = s.text.clone();
                            second_background = s.background;
                            second_reveal = s.text.chars().count();
                            voice = Some(s);
                        }
                        let partner = voice.filter(|s| !s.background);

                        // Word by word is decided per line: a source can tag
                        // some lines and not others. A duet partner without
                        // timings keeps both phrases whole, since one word
                        // stacked under a whole phrase reads as a mistake.
                        let split = app.cfg.word_by_word
                            && !line.words.is_empty()
                            && partner.is_none_or(|p| !p.words.is_empty());

                        // The second voice gets the line's split, and a duet
                        // partner its sweep; a background vocal is never swept.
                        if let Some(s) = voice {
                            second_highlight = if sweep {
                                timeline.second_highlight_chars(index, pos)
                            } else {
                                0
                            };
                            if split && !s.words.is_empty() {
                                match timeline.second_active_word(index, pos) {
                                    Some(word) => {
                                        second_highlight =
                                            second_highlight.saturating_sub(word.range.start);
                                        second_reveal = word.revealed();
                                        second_text = second_text
                                            .chars()
                                            .skip(word.range.start)
                                            .take(word.range.len())
                                            .collect();
                                    }
                                    None => second_text.clear(),
                                }
                            }
                        }

                        match split {
                            // The whole word goes to the renderer, so it is
                            // laid out at its final size and does not crawl
                            // sideways as its syllables fill in.
                            true => {
                                // The line has started but its first word has
                                // not: hold the screen rather than flash the
                                // whole line for a frame.
                                if let Some(word) = timeline.active_word(index, pos) {
                                    // The highlight is an offset into the whole
                                    // line; rebase it onto the word on screen.
                                    highlight = highlight.saturating_sub(word.range.start);
                                    reveal = word.revealed();
                                    line_text = line
                                        .text
                                        .chars()
                                        .skip(word.range.start)
                                        .take(word.range.len())
                                        .collect();
                                }
                            }
                            false => {
                                line_text = line.text.clone();
                                reveal = line.char_len();
                            }
                        }
                    }
                    // Recased here rather than in the renderer: the mapping is
                    // one character for one, so the offsets above still hold.
                    line_text = app.cfg.case.apply(&line_text);
                    second_text = app.cfg.case.apply(&second_text);
                    Screen::Lyric {
                        text: &line_text,
                        highlight,
                        reveal,
                        second: (!second_text.is_empty()).then_some(SecondVoice {
                            text: &second_text,
                            background: second_background,
                            highlight: second_highlight,
                            reveal: second_reveal,
                        }),
                    }
                }
                // During the intro and the outro, show whose song it is.
                Position::Intro { .. } | Position::Outro => Screen::Idle { message: &label },
            },
        };

        let lyric_height = area.height.saturating_sub(1);
        let text = crate::render::render(&screen, &app.font, area.width, lyric_height, app.theme);
        // Where the lyrics end: the text is padded at the top to centre it and
        // not at the bottom, so its length is the row under its last line.
        let lyric_bottom = u16::try_from(text.lines.len())
            .unwrap_or(u16::MAX)
            .min(lyric_height);
        frame.render_widget(Paragraph::new(text), area);

        // The bars stand on the status line and reach halfway up the empty
        // space between it and the lyrics, so they never crowd the text.
        if let Some(c) = cava {
            let rows = (lyric_height - lyric_bottom) / 2;
            if rows > 0 {
                let strip = ratatui::layout::Rect {
                    y: area.y + lyric_height - rows,
                    height: rows,
                    ..area
                };
                let text = bars::render(
                    &c.levels(),
                    cava::MAX_LEVEL,
                    usize::from(rows),
                    usize::from(area.width),
                    app.theme.dim,
                );
                frame.render_widget(Paragraph::new(text), strip);
            }
        }

        if let Some(status) = status_line(app)
            && area.height > 0
        {
            let strip = ratatui::layout::Rect {
                x: area.x,
                y: area.y + area.height - 1,
                width: area.width,
                height: 1,
            };
            frame.render_widget(Paragraph::new(status).style(app.theme.dim), strip);
        }
    })?;
    Ok(())
}

/// Just where the lyrics came from, or the notice after a keypress: everything
/// else is on screen already or in `lyrics status`.
fn status_line(app: &App) -> Option<String> {
    if let Some(notice) = app.notice_text() {
        return Some(format!(" {notice}"));
    }
    match &app.state {
        LyricState::Ready { source, .. } if app.cfg.show_source => {
            Some(format!(" {}", source.short()))
        }
        _ => None,
    }
}

/// What is already known is shown at once, and the full race still runs behind
/// it: a provider ahead of the cached one may have an answer by now.
fn start_lookup(
    app: &mut App,
    track: Option<Track>,
    lookup: &Arc<Engine>,
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
        app.state = ready_from(found);
        return;
    }

    app.state = match crate::lyrics::cached(lookup, &app.cfg.providers, &track) {
        Some(Outcome::Found(found)) => ready_from(*found),
        _ => LyricState::Searching,
    };

    let lookup = lookup.clone();
    let tx = tx.clone();
    let order = app.cfg.providers.clone();
    let network = app.cfg.network;
    tokio::spawn(async move {
        let key = crate::lyrics::lookup_key(&track);
        let send = |outcome| {
            let _ = tx.send(FetchResult {
                track_id: track.id.clone(),
                key: key.clone(),
                outcome,
            });
        };
        let outcome = crate::lyrics::lookup_with(&lookup, &order, &track, network, |early| {
            if let Outcome::Found(_) = early {
                send(Ok(early));
            }
        })
        .await
        .map_err(anyhow::Error::from);
        send(outcome);
    });
}

fn apply_fetch(app: &mut App, result: FetchResult) {
    // Ignore an answer for a track we have already moved on from.
    let current = app
        .engine
        .track()
        .map(|t| (t.id.clone(), crate::lyrics::lookup_key(t)));
    if current != Some((result.track_id, result.key)) {
        return;
    }
    let showing = matches!(app.state, LyricState::Ready { .. });
    match result.outcome {
        Ok(Outcome::Found(found)) => {
            if !matches!(&app.state, LyricState::Ready { raw, .. } if *raw == found.raw) {
                app.state = ready_from(*found);
            }
        }
        // Lyrics already on screen came from the cache; nothing better turned
        // up, and a failure to ask again is no reason to take them away.
        Ok(Outcome::Missing) if showing => {}
        Ok(Outcome::Missing) => app.state = LyricState::Missing,
        Err(_) if showing => {}
        Err(e) => {
            app.state = LyricState::Missing;
            app.note(format!("lookup failed: {e}"));
        }
    }
}

/// What `,` and `.` move the sync by.
const FINE_NUDGE_MS: i64 = 100;

/// What `<` and `>` move it by: an answer timed against another master is two
/// or three seconds out, thirty presses of the fine step.
const COARSE_NUDGE_MS: i64 = 1000;

/// Shared by all four nudge keys so the message and the save cannot drift
/// apart.
fn nudge(app: &mut App, delta_ms: i64) {
    app.nudge_offset(delta_ms);
    let off = app.offset_ms();
    app.note(format!("offset {off:+}ms  (saved for this song)"));
}

async fn handle_key(
    app: &mut App,
    key: KeyEvent,
    player: Option<&PlayerHandle>,
    lookup: &Arc<Engine>,
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
        KeyCode::Char(',') => nudge(app, -FINE_NUDGE_MS),
        KeyCode::Char('.') => nudge(app, FINE_NUDGE_MS),
        KeyCode::Char('<') => nudge(app, -COARSE_NUDGE_MS),
        KeyCode::Char('>') => nudge(app, COARSE_NUDGE_MS),
        KeyCode::Char('0') => {
            app.reset_offset();
            let off = app.offset_ms();
            app.note(format!("offset {off:+}ms  (back to the default)"));
        }
        KeyCode::Char('f') => {
            let next = font::next_after(app.font.name);
            app.font = font::by_name(next).unwrap_or_else(font::block);
            app.cfg.font = next.to_string();
            app.remember("font", next, format!("font: {next}"));
        }
        KeyCode::Char('c') => {
            app.cfg.case = app.cfg.case.next();
            let case = app.cfg.case.label();
            app.remember("case", case, format!("case: {case}"));
        }
        KeyCode::Char('s') => {
            app.cfg.sweep = app.cfg.sweep.next();
            let active = matches!(
                app.state,
                LyricState::Ready { word_timed, .. }
                    if app.cfg.sweep.applies(word_timed, app.cfg.word_by_word)
            );
            let sweep = app.cfg.sweep.label();
            app.remember(
                "sweep",
                sweep,
                format!("highlight: {sweep} ({})", if active { "on" } else { "off" }),
            );
        }
        KeyCode::Char('w') => {
            app.cfg.word_by_word = !app.cfg.word_by_word;
            let msg = if app.cfg.word_by_word {
                "one word at a time"
            } else {
                "whole lines"
            };
            app.remember("word_by_word", app.cfg.word_by_word, msg);
        }
        KeyCode::Char('v' | 'V') => {
            app.cfg.visualizer = !app.cfg.visualizer;
            let msg = match (&app.cava_error, app.cfg.visualizer) {
                (Some(e), true) => e.clone(),
                (None, true) => "visualizer on".to_string(),
                (_, false) => "visualizer off".to_string(),
            };
            app.remember("visualizer", app.cfg.visualizer, msg);
        }
        KeyCode::Char('h') => {
            app.cfg.show_source = !app.cfg.show_source;
            let msg = if app.cfg.show_source {
                "source shown"
            } else {
                "source hidden"
            };
            app.remember("show_source", app.cfg.show_source, msg);
        }
        KeyCode::Char('r') => {
            if let Some(track) = app.engine.track().cloned() {
                if !app.cfg.network {
                    app.note("network disabled; nothing to refetch from");
                    return;
                }
                let req = crate::lyrics::request_for(&track, true);
                lookup.forget(&req, &app.cfg.providers);
                app.note("refetching");
                app.state = LyricState::Searching;
                start_lookup(app, Some(track), lookup, tx);
            }
        }
        KeyCode::Char(' ') => {
            // The D-Bus connection is already open, so this costs nothing extra.
            match player {
                Some(player) => {
                    if let Err(e) = player.play_pause().await {
                        app.note(format!("play/pause failed: {e}"));
                    }
                }
                None => app.note("no player open"),
            }
        }
        _ => {}
    }
}
