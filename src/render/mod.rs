//! Composing the screen.
//!
//! Everything here is pure: state in, a ratatui `Text` out. The terminal is
//! only touched by `tui.rs`, so the layout can be unit-tested.

pub mod font;
pub mod layout;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

use self::font::Font;
use self::layout::Layout;

/// Blank rows between wrapped lines of block art.
pub const LINE_GAP: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub sung: Color,
    pub unsung: Color,
    pub dim: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // Deliberately terminal palette colours, not fixed RGB: the display
            // then matches whatever scheme the user's terminal is set to.
            sung: Color::Cyan,
            unsung: Color::White,
            dim: Color::DarkGray,
        }
    }
}

/// What the screen should show right now.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen<'a> {
    /// A lyric line.
    ///
    /// `highlight` is how many characters the sweep has passed. `reveal` is how
    /// many are drawn at all: a word timed in syllables is laid out whole, so it
    /// sits where it will finally sit, but only the syllables reached so far are
    /// inked. The rest holds its space blank instead of shifting the word.
    Lyric {
        text: &'a str,
        highlight: usize,
        reveal: usize,
    },
    /// Track known, lyrics being looked up.
    Searching { label: &'a str },
    /// Looked up, nothing found.
    NoLyrics { label: &'a str },
    /// Nothing is playing.
    Idle { message: &'a str },
}

/// Render a screen into styled text sized for `width` x `height`.
pub fn render(screen: &Screen<'_>, font: &Font, width: u16, height: u16, theme: Theme) -> Text<'static> {
    let width = width as usize;
    let height = height as usize;

    match screen {
        Screen::Lyric {
            text,
            highlight,
            reveal,
        } => {
            if text.trim().is_empty() {
                return Text::default();
            }
            let (layout, used) = layout::layout_fitting(text, font, width, height, LINE_GAP);
            block_text(&layout, &used, *highlight, *reveal, width, height, theme)
        }
        Screen::Searching { label } => status_text(label, "searching for lyrics", width, height, theme),
        Screen::NoLyrics { label } => status_text(label, "no lyrics found", width, height, theme),
        Screen::Idle { message } => status_text("", message, width, height, theme),
    }
}

fn block_text(
    layout: &Layout,
    font: &Font,
    highlight: usize,
    reveal: usize,
    width: usize,
    height: usize,
    theme: Theme,
) -> Text<'static> {
    let mut rows: Vec<Line<'static>> = Vec::new();

    for (i, vline) in layout.lines.iter().enumerate() {
        if i > 0 {
            for _ in 0..LINE_GAP {
                rows.push(Line::default());
            }
        }
        let pad = width.saturating_sub(vline.width) / 2;
        for row in 0..font.height {
            let mut spans: Vec<Span<'static>> = Vec::new();
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            for (j, cell) in vline.cells.iter().enumerate() {
                if j > 0 && font.tracking() > 0 {
                    spans.push(Span::raw(" ".repeat(font.tracking())));
                }
                let art = cell.rows.get(row).cloned().unwrap_or_default();
                // Not reached yet: hold the space, draw nothing in it.
                let art = match cell.src < reveal {
                    true => pad_to(&art, cell.width),
                    false => " ".repeat(cell.width),
                };
                let style = if cell.src < highlight {
                    Style::default().fg(theme.sung).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.unsung)
                };
                spans.push(Span::styled(art, style));
            }
            rows.push(Line::from(spans));
        }
    }

    vertically_centre(rows, height)
}

/// Glyph rows can be shorter than the glyph's nominal width (trailing spaces
/// trimmed in the tables); pad so columns stay aligned.
fn pad_to(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

fn status_text(label: &str, message: &str, width: usize, height: usize, theme: Theme) -> Text<'static> {
    let mut rows: Vec<Line<'static>> = Vec::new();
    if !label.is_empty() {
        rows.push(centred(label, width, Style::default().fg(theme.unsung)));
        rows.push(Line::default());
    }
    rows.push(centred(message, width, Style::default().fg(theme.dim)));
    vertically_centre(rows, height)
}

fn centred(text: &str, width: usize, style: Style) -> Line<'static> {
    let len = text.chars().count();
    let pad = width.saturating_sub(len) / 2;
    Line::from(vec![
        Span::raw(" ".repeat(pad)),
        Span::styled(text.to_string(), style),
    ])
}

fn vertically_centre(rows: Vec<Line<'static>>, height: usize) -> Text<'static> {
    if rows.len() >= height {
        return Text::from(rows);
    }
    let pad = (height - rows.len()) / 2;
    let mut out: Vec<Line<'static>> = Vec::with_capacity(height);
    out.extend(std::iter::repeat_n(Line::default(), pad));
    out.extend(rows);
    Text::from(out)
}
