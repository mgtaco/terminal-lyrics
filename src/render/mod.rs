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

/// Blank rows between the two voices. Any tighter and they read as one phrase
/// that happened to wrap, rather than as two people singing.
pub const VOICE_GAP: usize = 1;

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

/// A second voice on screen alongside the line being read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SecondVoice<'a> {
    pub text: &'a str,
    /// A background vocal: dimmed, and drawn a size smaller than the line it
    /// sits over. The other half of a duet is neither — it is a co-equal voice.
    pub background: bool,
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
        /// Drawn above `text`, and always drawn whole: the sweep and the
        /// word-by-word split belong to the line being read.
        second: Option<SecondVoice<'a>>,
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
            second,
        } => {
            // A line that is nothing but a background vocal has no main text,
            // and must still draw the voice that is actually singing.
            if text.trim().is_empty() && second.is_none_or(|s| s.text.trim().is_empty()) {
                return Text::default();
            }
            let (main, second) = fit_voices(text, *second, font, width, height);
            let mut rows: Vec<Line<'static>> = Vec::new();
            if let Some(s) = &second {
                // Grey for a background vocal; the same colour the main line's
                // unsung characters use for a duet partner, so the two read as
                // equals rather than as a line and its shadow.
                let colour = match s.background {
                    true => theme.dim,
                    false => theme.unsung,
                };
                rows.extend(block_rows(
                    &s.fitted.layout,
                    &s.fitted.font,
                    RowStyle::Flat(colour),
                    width,
                    theme,
                ));
                if !main.layout.is_empty() {
                    rows.extend(std::iter::repeat_n(Line::default(), VOICE_GAP));
                }
            }
            rows.extend(block_rows(
                &main.layout,
                &main.font,
                RowStyle::Sweep {
                    highlight: *highlight,
                    reveal: *reveal,
                },
                width,
                theme,
            ));
            vertically_centre(rows, height)
        }
        Screen::Searching { label } => status_text(label, "searching for lyrics", width, height, theme),
        Screen::NoLyrics { label } => status_text(label, "no lyrics found", width, height, theme),
        Screen::Idle { message } => status_text("", message, width, height, theme),
    }
}

/// How a run of glyph art is coloured.
enum RowStyle {
    /// The line being read: swept characters bold in `sung`, the rest in
    /// `unsung`, and only the first `reveal` inked at all.
    Sweep { highlight: usize, reveal: usize },
    /// A second voice: one colour, all of it inked, never bold.
    Flat(Color),
}

/// One voice laid out at the size it will be drawn.
struct Fitted {
    layout: Layout,
    font: Font,
}

/// A fitted second voice, and whether it is a background vocal.
struct FittedSecond {
    fitted: Fitted,
    background: bool,
}

/// Fit the line, and the voice over it, into the space there is.
///
/// The order matters. The line's own size is settled first, against the whole
/// budget, and is the floor: the pair may step down together to make room, but
/// if even the smallest font cannot hold both, the second voice is dropped and
/// the line goes back to the size it would have had alone. Shrinking the lyric
/// for something that then gets dropped would be the worst of both.
fn fit_voices(
    main: &str,
    second: Option<SecondVoice<'_>>,
    preferred: &Font,
    width: usize,
    height: usize,
) -> (Fitted, Option<FittedSecond>) {
    let (layout, font) = layout::layout_fitting(main, preferred, width, height, LINE_GAP);
    let solo = Fitted { layout, font };

    let Some(second) = second.filter(|s| !s.text.trim().is_empty()) else {
        return (solo, None);
    };

    // Never larger than the line manages on its own: a font that cannot hold
    // one voice is not going to hold two.
    for font in layout::fallback_chain(&solo.font) {
        let second_font = match second.background {
            true => font::smaller_than(font.name).unwrap_or_else(|| font.clone()),
            false => font.clone(),
        };
        let lm = layout::layout(main, &font, width);
        let ls = layout::layout(second.text, &second_font, width);
        let mut rows = ls.rows(LINE_GAP);
        if !lm.is_empty() {
            rows += VOICE_GAP + lm.rows(LINE_GAP);
        }
        if rows <= height.max(1) {
            return (
                Fitted { layout: lm, font },
                Some(FittedSecond {
                    fitted: Fitted {
                        layout: ls,
                        font: second_font,
                    },
                    background: second.background,
                }),
            );
        }
    }

    (solo, None)
}

/// Rows of glyph art for one voice, centred horizontally but not vertically —
/// the two voices are centred together, as one block.
fn block_rows(
    layout: &Layout,
    font: &Font,
    style: RowStyle,
    width: usize,
    theme: Theme,
) -> Vec<Line<'static>> {
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
                let (reached, style) = match style {
                    RowStyle::Sweep { highlight, reveal } => (
                        cell.src < reveal,
                        if cell.src < highlight {
                            Style::default().fg(theme.sung).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme.unsung)
                        },
                    ),
                    RowStyle::Flat(colour) => (true, Style::default().fg(colour)),
                };
                // Not reached yet: hold the space, draw nothing in it.
                let art = match reached {
                    true => pad_to(&art, cell.width),
                    false => " ".repeat(cell.width),
                };
                spans.push(Span::styled(art, style));
            }
            rows.push(Line::from(spans));
        }
    }

    rows
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
