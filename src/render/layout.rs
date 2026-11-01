//! Lines wrap at word boundaries and the font steps down before anything is
//! cut; nothing is ever truncated.

use unicode_script::{Script, UnicodeScript};

use super::font::{Font, Glyph};

/// One rendered character: rows of art plus where it came from in the source
/// line, which is what lets the sweep highlight partial words.
#[derive(Debug, Clone)]
pub struct Cell {
    pub rows: Vec<String>,
    pub width: usize,
    /// Index of this character within the source line. A combining mark
    /// drawn onto the letter before it shares that letter's cell and index.
    pub src: usize,
}

/// A wrapped visual line: cells that together fit the available width.
#[derive(Debug, Clone, Default)]
pub struct VisualLine {
    pub cells: Vec<Cell>,
    pub width: usize,
    /// Rows on screen, including any above the capitals or below the baseline.
    pub height: usize,
    /// Rows above the cap line: room an accent on a capital needed.
    pub above: usize,
    /// Rows below the baseline: room a descender needed.
    pub below: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub lines: Vec<VisualLine>,
    pub width: usize,
    /// Blank columns between neighbouring cells.
    pub tracking: usize,
}

impl Layout {
    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.cells.is_empty())
    }

    /// Total rows on screen, including the blank rows between wrapped lines.
    pub fn rows(&self, gap: usize) -> usize {
        let art: usize = self.lines.iter().map(|l| l.height).sum();
        art + gap * self.lines.len().saturating_sub(1)
    }
}

/// A character drawn but not yet wrapped or packed into rows.
struct Piece {
    glyph: Glyph,
    src: usize,
    /// Whitespace in the source, where a line may break.
    space: bool,
    /// Whether a line may break just before this, though no space comes first.
    breaks: bool,
}

impl Piece {
    fn width(&self) -> usize {
        self.glyph.width()
    }
}

/// Characters of `text` with the combining marks that follow each folded into
/// it, so a lyric in decomposed form draws the same as a precomposed one.
fn pieces(text: &str, font: &Font) -> Vec<Piece> {
    let mut clusters: Vec<(usize, char, Vec<char>)> = Vec::new();
    for (i, c) in text.chars().enumerate() {
        match clusters.last_mut() {
            Some((_, base, marks)) if super::font::is_mark(c) && !base.is_whitespace() => {
                marks.push(c)
            }
            _ => clusters.push((i, c, Vec::new())),
        }
    }
    let mut prev = ' ';
    clusters
        .into_iter()
        .map(|(src, c, marks)| {
            let breaks = breaks_between(prev, c);
            prev = c;
            Piece {
                glyph: font.draw(c, &marks),
                src,
                space: c.is_whitespace(),
                breaks,
            }
        })
        .collect()
}

/// Chinese and Japanese are written without spaces, so a line may break
/// between any two Han or kana characters, though not so as to start a line
/// with a small kana or a length mark, or end one on an opening bracket.
fn breaks_between(prev: char, c: char) -> bool {
    const OPENING: &str = "「『（【〈《〔〖［｛";
    let starts = OPENING.contains(c)
        || matches!(
            c.script(),
            Script::Han | Script::Hiragana | Script::Katakana
        ) && !"ぁぃぅぇぉっゃゅょゎゕゖァィゥェォッャュョヮヵヶーｰ々ゝゞヽヾ".contains(c);
    starts && !OPENING.contains(prev)
}

/// A word wider than the terminal breaks mid-word rather than being clipped. A
/// line mostly in letters the font cannot draw is laid out in `mini` instead.
pub fn layout(text: &str, font: &Font, max_width: usize) -> Layout {
    if !font.is_plain() {
        let letters = text
            .chars()
            .filter(|c| c.is_alphabetic() && !super::font::is_mark(*c));
        let (all, foreign) = letters.fold((0, 0), |(all, foreign), c| {
            (all + 1, foreign + usize::from(!font.can_draw(c)))
        });
        if foreign > 0 && foreign * 2 >= all {
            return layout(text, &super::font::mini(), max_width);
        }
    }

    let max_width = max_width.max(1);
    let tracking = font.tracking();

    let mut lines: Vec<Vec<Piece>> = Vec::new();
    let mut current: Vec<Piece> = Vec::new();
    let mut current_width = 0usize;
    // Pieces of the word being built, so it can move to the next line whole.
    let mut word: Vec<Piece> = Vec::new();
    let mut word_width = 0usize;

    let flush_word = |current: &mut Vec<Piece>,
                      current_width: &mut usize,
                      lines: &mut Vec<Vec<Piece>>,
                      word: &mut Vec<Piece>,
                      word_width: &mut usize| {
        if word.is_empty() {
            return;
        }
        let advance = if current.is_empty() {
            *word_width
        } else {
            *word_width + tracking
        };
        if !current.is_empty() && *current_width + advance > max_width {
            lines.push(std::mem::take(current));
            *current_width = 0;
        }
        for piece in word.drain(..) {
            let step = if current.is_empty() {
                piece.width()
            } else {
                piece.width() + tracking
            };
            // A single word longer than the line: break it rather than lose it.
            if !current.is_empty() && *current_width + step > max_width {
                lines.push(std::mem::take(current));
                *current_width = piece.width();
            } else {
                *current_width += step;
            }
            current.push(piece);
        }
        *word_width = 0;
    };

    for piece in pieces(text, font) {
        if piece.space {
            flush_word(
                &mut current,
                &mut current_width,
                &mut lines,
                &mut word,
                &mut word_width,
            );
            // The space itself is a cell, so highlight offsets stay aligned
            // with the source text — but never at the start of a wrapped line.
            if !current.is_empty() {
                let step = piece.width() + tracking;
                if current_width + step <= max_width {
                    current_width += step;
                    current.push(piece);
                }
            }
            continue;
        }
        if piece.breaks {
            flush_word(
                &mut current,
                &mut current_width,
                &mut lines,
                &mut word,
                &mut word_width,
            );
        }
        word_width += if word.is_empty() {
            piece.width()
        } else {
            piece.width() + tracking
        };
        word.push(piece);
    }
    flush_word(
        &mut current,
        &mut current_width,
        &mut lines,
        &mut word,
        &mut word_width,
    );
    if !current.is_empty() {
        lines.push(current);
    }

    // Trailing spaces at the end of a wrapped line would centre it off-kilter.
    for line in &mut lines {
        while line.last().is_some_and(|p| p.glyph.is_blank()) {
            line.pop();
        }
    }
    lines.retain(|l| !l.is_empty());

    let lines: Vec<VisualLine> = lines
        .into_iter()
        .map(|pieces| pack(pieces, font, tracking))
        .collect();
    let width = lines.iter().map(|l| l.width).max().unwrap_or(0);
    Layout {
        lines,
        width,
        tracking,
    }
}

/// Turn a wrapped line of glyphs into rows of terminal text.
fn pack(pieces: Vec<Piece>, font: &Font, tracking: usize) -> VisualLine {
    let width =
        pieces.iter().map(Piece::width).sum::<usize>() + tracking * pieces.len().saturating_sub(1);

    if font.is_plain() {
        let cells = pieces
            .into_iter()
            .map(|p| {
                let width = p.width();
                let text = match p.glyph {
                    Glyph::Plain(s) => s,
                    Glyph::Art(_) => " ".repeat(width),
                };
                Cell {
                    rows: vec![text],
                    width,
                    src: p.src,
                }
            })
            .collect();
        return VisualLine {
            cells,
            width,
            height: 1,
            above: 0,
            below: 0,
        };
    }

    // Always the capitals' rows, so a line of nothing but dashes or dots
    // stands where a line of letters would; then whatever overhangs them.
    let cap = font.cap_px() as i32;
    let (mut top, mut bottom) = (0i32, cap);
    for piece in &pieces {
        if let Glyph::Art(b) = &piece.glyph
            && let Some((first, last)) = b.ink_rows()
        {
            top = top.min(first);
            bottom = bottom.max(last + 1);
        }
    }
    // Onto the row grid, which the cap line and the baseline both sit on.
    let top = top.div_euclid(2) * 2;
    let bottom = bottom + bottom.rem_euclid(2);
    let height = ((bottom - top) / 2) as usize;
    let above = (-top / 2) as usize;
    let below = ((bottom - cap) / 2) as usize;
    // A character with no art sits on the middle row of the capitals.
    let plain_row = above + font.height / 2;

    let cells = pieces
        .into_iter()
        .map(|p| {
            let width = p.width();
            let rows = match &p.glyph {
                Glyph::Art(b) => (0..height)
                    .map(|r| {
                        let y = top + 2 * r as i32;
                        (0..width)
                            .map(|x| match (b.ink(x, y), b.ink(x, y + 1)) {
                                (false, false) => ' ',
                                (true, false) => '▀',
                                (false, true) => '▄',
                                (true, true) => '█',
                            })
                            .collect()
                    })
                    .collect(),
                Glyph::Plain(s) => (0..height)
                    .map(|r| match r == plain_row {
                        true => s.clone(),
                        false => " ".repeat(width),
                    })
                    .collect(),
            };
            Cell {
                rows,
                width,
                src: p.src,
            }
        })
        .collect();
    VisualLine {
        cells,
        width,
        height,
        above,
        below,
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
pub(crate) fn fallback_chain(preferred: &Font) -> Vec<Font> {
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
