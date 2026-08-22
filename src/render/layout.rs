//! Turning one lyric line into rows of glyph art that fit the terminal.
//!
//! v1 rendered the whole line and then cut it to the terminal width, so a long
//! line simply lost its end. Here the glyph run is wrapped at word boundaries,
//! and if a single word is still too wide the font steps down before anything
//! is discarded. Nothing is ever truncated.

use super::font::Font;

/// One rendered character: `height` rows of art plus where it came from in the
/// source line, which is what lets the sweep highlight partial words.
#[derive(Debug, Clone)]
pub struct Cell {
    pub rows: Vec<String>,
    pub width: usize,
    /// Index of this character within the source line.
    pub src: usize,
}

/// A wrapped visual line: cells that together fit the available width.
#[derive(Debug, Clone, Default)]
pub struct VisualLine {
    pub cells: Vec<Cell>,
    pub width: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub lines: Vec<VisualLine>,
    pub height: usize,
    pub width: usize,
}

impl Layout {
    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.cells.is_empty())
    }

    /// Total rows on screen, including the blank row between wrapped lines.
    pub fn rows(&self, gap: usize) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        self.lines.len() * self.height + gap * self.lines.len().saturating_sub(1)
    }
}

fn cell_for(font: &Font, c: char, src: usize) -> Cell {
    Cell {
        rows: font.glyph(c),
        width: font.width_of(c),
        src,
    }
}

/// Wrap `text` to `max_width` columns using `font`.
///
/// Breaks between words where possible. A word wider than the whole terminal is
/// broken mid-word rather than clipped — ugly, but it keeps every character.
pub fn layout(text: &str, font: &Font, max_width: usize) -> Layout {
    let max_width = max_width.max(1);
    let tracking = font.tracking();

    let mut lines: Vec<VisualLine> = Vec::new();
    let mut current = VisualLine::default();
    // Cells of the word being built, so it can move to the next line whole.
    let mut word: Vec<Cell> = Vec::new();
    let mut word_width = 0usize;

    let flush_word = |current: &mut VisualLine,
                      lines: &mut Vec<VisualLine>,
                      word: &mut Vec<Cell>,
                      word_width: &mut usize| {
        if word.is_empty() {
            return;
        }
        let advance = if current.cells.is_empty() {
            *word_width
        } else {
            *word_width + tracking
        };
        if !current.cells.is_empty() && current.width + advance > max_width {
            lines.push(std::mem::take(current));
        }
        for cell in word.drain(..) {
            let step = if current.cells.is_empty() {
                cell.width
            } else {
                cell.width + tracking
            };
            // A single word longer than the line: break it rather than lose it.
            if !current.cells.is_empty() && current.width + step > max_width {
                lines.push(std::mem::take(current));
                current.width = cell.width;
            } else {
                current.width += step;
            }
            current.cells.push(cell);
        }
        *word_width = 0;
    };

    for (i, c) in text.chars().enumerate() {
        if c.is_whitespace() {
            flush_word(&mut current, &mut lines, &mut word, &mut word_width);
            // The space itself is a cell, so highlight offsets stay aligned
            // with the source text — but never at the start of a wrapped line.
            if !current.cells.is_empty() {
                let cell = cell_for(font, ' ', i);
                let step = cell.width + tracking;
                if current.width + step <= max_width {
                    current.width += step;
                    current.cells.push(cell);
                }
            }
            continue;
        }
        let cell = cell_for(font, c, i);
        word_width += if word.is_empty() {
            cell.width
        } else {
            cell.width + tracking
        };
        word.push(cell);
    }
    flush_word(&mut current, &mut lines, &mut word, &mut word_width);
    if !current.cells.is_empty() {
        lines.push(current);
    }

    // Trailing spaces at the end of a wrapped line would centre it off-kilter.
    for line in &mut lines {
        while line
            .cells
            .last()
            .is_some_and(|c| c.rows.iter().all(|r| r.trim().is_empty()))
        {
            let c = line.cells.pop().expect("checked by is_some_and");
            line.width = line.width.saturating_sub(c.width + tracking);
        }
    }
    lines.retain(|l| !l.cells.is_empty());

    let width = lines.iter().map(|l| l.width).max().unwrap_or(0);
    Layout {
        lines,
        height: font.height,
        width,
    }
}

/// Lay `text` out with the largest font that fits `width` x `height`, stepping
/// down through the font list rather than clipping.
pub fn layout_fitting(
    text: &str,
    preferred: &Font,
    width: usize,
    height: usize,
    gap: usize,
) -> (Layout, Font) {
    let candidates = fallback_chain(preferred);
    let mut last = None;
    for font in candidates {
        let l = layout(text, &font, width);
        if l.rows(gap) <= height.max(1) {
            return (l, font);
        }
        last = Some((l, font));
    }
    // Even the smallest font overflows: show it anyway, scrolled to the sweep,
    // which the caller handles. Still no truncation.
    last.unwrap_or_else(|| (Layout::default(), super::font::mini()))
}

/// Preferred font first, then progressively smaller ones.
fn fallback_chain(preferred: &Font) -> Vec<Font> {
    let mut out = vec![preferred.clone()];
    for name in ["compact", "mini"] {
        if name != preferred.name
            && let Some(f) = super::font::by_name(name)
            && f.height < preferred.height
        {
            out.push(f);
        }
    }
    out
}
