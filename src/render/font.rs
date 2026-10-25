//! The look comes from
//! [tacos-terminal-lyrics](https://github.com/tacoproz1/tacos-terminal-lyrics).
//! An accented letter is never drawn by hand: its base letter gets the marks
//! stamped on.

use std::collections::HashMap;
use std::sync::LazyLock;

use unicode_normalization::char::{decompose_canonical, is_combining_mark};
use unicode_width::UnicodeWidthStr;

/// `top` is the first row's y in pixels down from the cap line; negative is a
/// mark standing above the capitals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub width: usize,
    pub top: i32,
    rows: Vec<Vec<bool>>,
}

impl Bitmap {
    fn blank(width: usize, top: i32, height: usize) -> Self {
        Self {
            width,
            top,
            rows: vec![vec![false; width]; height],
        }
    }

    /// Whether the pixel at column `x`, row `y` (same frame as `top`) is inked.
    /// Anything outside the bitmap is blank.
    pub fn ink(&self, x: usize, y: i32) -> bool {
        let Ok(row) = usize::try_from(y - self.top) else {
            return false;
        };
        self.rows
            .get(row)
            .and_then(|r| r.get(x))
            .copied()
            .unwrap_or(false)
    }

    pub fn is_blank(&self) -> bool {
        self.rows.iter().all(|r| r.iter().all(|p| !p))
    }

    /// First and last inked row, in the frame of `top`.
    pub fn ink_rows(&self) -> Option<(i32, i32)> {
        let inked = |(_, r): &(usize, &Vec<bool>)| r.iter().any(|p| *p);
        let first = self.rows.iter().enumerate().find(inked)?.0;
        let last = self.rows.iter().enumerate().rfind(inked)?.0;
        Some((self.top + first as i32, self.top + last as i32))
    }

    /// First and last inked column.
    fn ink_cols(&self) -> Option<(usize, usize)> {
        let inked = |x: &usize| self.rows.iter().any(|r| r[*x]);
        let first = (0..self.width).find(inked)?;
        let last = (0..self.width).rfind(inked)?;
        Some((first, last))
    }

    /// Grows the canvas wherever `mark` overhangs it, so a mark wider than its
    /// letter widens the glyph rather than being cut.
    fn stamp(&mut self, mark: &Bitmap, x: i32, y: i32) {
        if x < 0 {
            let grow = (-x) as usize;
            for row in &mut self.rows {
                row.splice(0..0, std::iter::repeat_n(false, grow));
            }
            self.width += grow;
            return self.stamp(mark, 0, y);
        }
        let x = x as usize;
        if x + mark.width > self.width {
            self.width = x + mark.width;
            for row in &mut self.rows {
                row.resize(self.width, false);
            }
        }
        if y < self.top {
            let grow = (self.top - y) as usize;
            self.rows
                .splice(0..0, std::iter::repeat_n(vec![false; self.width], grow));
            self.top = y;
        }
        let bottom = y + mark.rows.len() as i32;
        let have = self.top + self.rows.len() as i32;
        if bottom > have {
            let grow = (bottom - have) as usize;
            self.rows
                .extend(std::iter::repeat_n(vec![false; self.width], grow));
        }
        for (dy, row) in mark.rows.iter().enumerate() {
            let target = &mut self.rows[(y - self.top) as usize + dy];
            for (dx, p) in row.iter().enumerate() {
                target[x + dx] |= *p;
            }
        }
    }
}

/// What one character of a lyric is drawn as.
#[derive(Debug, Clone, PartialEq)]
pub enum Glyph {
    Art(Bitmap),
    /// The character as terminal text: all of `mini`, and anything a block font
    /// has no art for, so a stray emoji is still shown.
    Plain(String),
}

impl Glyph {
    pub fn width(&self) -> usize {
        match self {
            Glyph::Art(b) => b.width,
            Glyph::Plain(s) => s.width(),
        }
    }

    pub fn is_blank(&self) -> bool {
        match self {
            Glyph::Art(b) => b.is_blank(),
            Glyph::Plain(s) => s.trim().is_empty(),
        }
    }
}

/// A parsed `.font` file.
#[derive(Debug)]
struct Face {
    /// Cap height in pixels: rows from the cap line down to the baseline.
    cap: usize,
    /// Pixels between a mark and the letter it sits on.
    gap: usize,
    /// Width of a space, in columns.
    space: usize,
    /// Blank columns between neighbouring glyphs.
    tracking: usize,
    glyphs: HashMap<char, Bitmap>,
    marks: HashMap<String, Bitmap>,
}

/// Header keys every `.font` file must set.
const HEADER: [&str; 5] = ["cap", "descent", "gap", "space", "tracking"];

impl Face {
    /// `key value` headers, then `== X` (or `== mark:name`) per glyph and its
    /// rows of `#` and `.`: `cap` rows, or `cap + descent`. `;` starts a
    /// comment line.
    fn parse(src: &str) -> Result<Self, String> {
        let mut header: HashMap<&str, usize> = HashMap::new();
        let mut entries: Vec<(&str, Vec<&str>)> = Vec::new();
        for (n, line) in src.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if let Some(name) = line.strip_prefix("== ") {
                entries.push((name, Vec::new()));
            } else if line.chars().all(|c| c == '#' || c == '.') {
                let Some((_, rows)) = entries.last_mut() else {
                    return Err(format!("line {}: art before any `==` entry", n + 1));
                };
                rows.push(line);
            } else if entries.is_empty()
                && let Some((key, value)) = line.split_once(' ')
                && HEADER.contains(&key)
            {
                let value = value
                    .trim()
                    .parse()
                    .map_err(|_| format!("line {}: `{key}` needs a number", n + 1))?;
                header.insert(key, value);
            } else {
                return Err(format!("line {}: cannot read {line:?}", n + 1));
            }
        }
        let get = |key: &str| {
            header
                .get(key)
                .copied()
                .ok_or_else(|| format!("missing header `{key}`"))
        };
        let (cap, descent) = (get("cap")?, get("descent")?);
        let mut face = Face {
            cap,
            gap: get("gap")?,
            space: get("space")?,
            tracking: get("tracking")?,
            glyphs: HashMap::new(),
            marks: HashMap::new(),
        };
        if cap % 2 != 0 || descent % 2 != 0 {
            return Err("`cap` and `descent` must be even, to sit on whole rows".into());
        }

        for (name, rows) in entries {
            let width = rows.first().map_or(0, |r| r.len());
            if width == 0 || rows.iter().any(|r| r.len() != width) {
                return Err(format!(
                    "`{name}`: rows must all be the same, non-zero width"
                ));
            }
            let mut bitmap = Bitmap {
                width,
                top: 0,
                rows: rows
                    .iter()
                    .map(|r| r.chars().map(|c| c == '#').collect())
                    .collect(),
            };
            if let Some(mark) = name.strip_prefix("mark:") {
                if face.marks.insert(mark.to_string(), bitmap).is_some() {
                    return Err(format!("mark `{mark}` is defined twice"));
                }
                continue;
            }
            let mut chars = name.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return Err(format!("`{name}` is not a single character"));
            };
            match bitmap.rows.len() {
                n if n == cap => bitmap.rows.resize(cap + descent, vec![false; width]),
                n if n == cap + descent => {}
                n => {
                    return Err(format!(
                        "`{c}` has {n} rows; a letter has {cap}, or {} with its descender",
                        cap + descent
                    ));
                }
            }
            if face.glyphs.insert(c, bitmap).is_some() {
                return Err(format!("`{c}` is defined twice"));
            }
        }
        Ok(face)
    }

    /// The glyph for a character with no marks on it, if the font has one. A
    /// lowercase letter the font does not draw borrows its capital.
    fn lookup(&self, c: char) -> Option<&Bitmap> {
        self.glyphs.get(&c).or_else(|| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(u), None) if u != c => self.glyphs.get(&u),
                _ => None,
            }
        })
    }

    /// `c` split into a letter this font draws and the marks to put on it, or
    /// `None` if the font has no way to draw it at all.
    fn split(&self, c: char) -> Option<(char, Vec<char>)> {
        if self.glyphs.contains_key(&c) {
            return Some((c, Vec::new()));
        }
        let mut parts = Vec::new();
        decompose_canonical(c, |d| parts.push(d));
        let base = parts[0];
        let marks = parts.split_off(1);
        if marks.iter().all(|m| is_combining_mark(*m)) && self.lookup(base).is_some() {
            return Some((base, marks));
        }
        self.lookup(c).map(|_| (c, Vec::new()))
    }

    fn draw(&self, c: char, extra: &[char]) -> Option<Bitmap> {
        if c.is_whitespace() {
            return Some(Bitmap::blank(self.space, 0, self.cap));
        }
        let (mut base, mut marks) = self.split(c)?;
        marks.extend_from_slice(extra);
        // A dot and an accent do not stack: í is a dotless i with an acute.
        if marks.iter().any(|m| placement(*m) == Some(Place::Above)) {
            let dotless = match base {
                // Cyrillic `і` (U+0456) is dotted too: ї is `і` plus a diaeresis.
                'i' | '\u{456}' => Some('ı'),
                'j' => Some('ȷ'),
                _ => None,
            };
            if let Some(d) = dotless.filter(|d| self.glyphs.contains_key(d)) {
                base = d;
            }
        }
        let mut bitmap = self.lookup(base)?.clone();
        for mark in marks {
            self.put_mark(&mut bitmap, base, mark);
        }
        Some(bitmap)
    }

    /// Stamp one combining mark onto `bitmap`. A mark the font has no art for
    /// is left off: the letter under it is still the right letter.
    fn put_mark(&self, bitmap: &mut Bitmap, base: char, mark: char) {
        let Some(place) = placement(mark) else {
            return;
        };
        let (name, place) = match (mark, base) {
            // On a letter with an ascender the caron is a small stroke beside
            // it, as in ď and ľ: there is no room for it on top.
            ('\u{30C}', 'd' | 'l' | 't' | 'L') => ("caron-side", Place::Side),
            // Latvian ģ carries its cedilla above, out of the descender's way.
            ('\u{327}', 'g') => ("comma-above", Place::Above),
            _ => (mark_name(mark), place),
        };
        let (Some(art), Some((x0, x1)), Some((top, bottom))) =
            (self.marks.get(name), bitmap.ink_cols(), bitmap.ink_rows())
        else {
            return;
        };
        let (x0, x1) = (x0 as i32, x1 as i32);
        let (w, h) = (art.width as i32, art.rows.len() as i32);
        let gap = self.gap as i32;
        let centred = x0 + (x1 - x0 + 1 - w).div_euclid(2);
        let (x, y) = match place {
            Place::Above => (centred, top - gap - h),
            Place::Below => (centred, bottom + 1 + gap),
            Place::Attached => (centred, bottom + 1),
            Place::Ogonek => (x1 + 1 - w, bottom + 1),
            Place::Side => (x1 + 2, top),
            Place::Horn => (x1 + 1, top - h + 2),
        };
        bitmap.stamp(art, x, y);
    }
}

/// Where a combining mark goes relative to the letter it sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// Centred over the letter, `gap` above its highest ink.
    Above,
    /// Centred under the letter, `gap` below its lowest ink.
    Below,
    /// Hanging straight off the bottom of the letter.
    Attached,
    /// Hanging off the bottom right corner.
    Ogonek,
    /// To the right of the letter's top.
    Side,
    /// Rising off the top right corner.
    Horn,
}

fn placement(mark: char) -> Option<Place> {
    Some(match mark {
        '\u{300}'..='\u{30C}' => Place::Above,
        '\u{31B}' => Place::Horn,
        '\u{323}' | '\u{326}' => Place::Below,
        '\u{327}' => Place::Attached,
        '\u{328}' => Place::Ogonek,
        _ => return None,
    })
}

fn mark_name(mark: char) -> &'static str {
    match mark {
        '\u{300}' => "grave",
        '\u{301}' => "acute",
        '\u{302}' => "circumflex",
        '\u{303}' => "tilde",
        '\u{304}' => "macron",
        '\u{305}' => "macron",
        '\u{306}' => "breve",
        '\u{307}' => "dot",
        '\u{308}' => "diaeresis",
        '\u{309}' => "hook",
        '\u{30A}' => "ring",
        '\u{30B}' => "double-acute",
        '\u{30C}' => "caron",
        '\u{31B}' => "horn",
        '\u{323}' => "dot",
        '\u{326}' => "comma-below",
        '\u{327}' => "cedilla",
        '\u{328}' => "ogonek",
        _ => "",
    }
}

/// Whether `c` combines onto the character before it rather than standing on
/// its own.
pub fn is_mark(c: char) -> bool {
    is_combining_mark(c)
}

#[derive(Debug, Clone)]
pub struct Font {
    pub name: &'static str,
    /// Rows from the cap line to the baseline: how tall a capital is. An
    /// accent or a descender adds rows to the line it is in, and only that one.
    pub height: usize,
    /// `None` for `mini`, which draws characters as themselves.
    face: Option<&'static Face>,
}

impl Font {
    fn from_face(name: &'static str, face: &'static Face) -> Self {
        Self {
            name,
            height: face.cap / 2,
            face: Some(face),
        }
    }

    /// Whether this font draws characters as themselves rather than as art.
    pub fn is_plain(&self) -> bool {
        self.face.is_none()
    }

    /// Cap height in pixels; zero for `mini`.
    pub fn cap_px(&self) -> usize {
        self.face.map_or(0, |f| f.cap)
    }

    /// Whether there is art for `c`, accents and all. `mini` draws everything.
    pub fn can_draw(&self, c: char) -> bool {
        match self.face {
            None => true,
            Some(face) => c.is_whitespace() || face.split(c).is_some(),
        }
    }

    /// `c` with any combining `marks` that followed it in the text.
    pub fn draw(&self, c: char, marks: &[char]) -> Glyph {
        let plain = || Glyph::Plain(std::iter::once(c).chain(marks.iter().copied()).collect());
        match self.face {
            None => plain(),
            Some(face) => face.draw(c, marks).map_or_else(plain, Glyph::Art),
        }
    }

    /// Columns inserted between adjacent glyphs. Block art needs the gap;
    /// plain text already has spaces in it.
    pub fn tracking(&self) -> usize {
        self.face.map_or(0, |f| f.tracking)
    }
}

static BLOCK: LazyLock<Face> = LazyLock::new(|| {
    Face::parse(include_str!("fonts/block.font")).unwrap_or_else(|e| panic!("block.font: {e}"))
});
static COMPACT: LazyLock<Face> = LazyLock::new(|| {
    Face::parse(include_str!("fonts/compact.font")).unwrap_or_else(|e| panic!("compact.font: {e}"))
});

pub fn by_name(name: &str) -> Option<Font> {
    match name.to_lowercase().as_str() {
        "block" => Some(block()),
        "compact" => Some(compact()),
        "mini" => Some(mini()),
        _ => None,
    }
}

/// Names in cycle order, for the `f` key.
pub const NAMES: &[&str] = &["block", "compact", "mini"];

pub fn next_after(name: &str) -> &'static str {
    let idx = NAMES.iter().position(|n| *n == name).unwrap_or(0);
    NAMES[(idx + 1) % NAMES.len()]
}

/// Unlike [`next_after`], which is the `f` key's cycle and wraps, stepping down
/// has to stop.
pub fn smaller_than(name: &str) -> Option<Font> {
    let idx = NAMES.iter().position(|n| *n == name)?;
    NAMES.get(idx + 1).and_then(|n| by_name(n))
}

/// Five rows a capital, two-pixel strokes.
pub fn block() -> Font {
    Font::from_face("block", &BLOCK)
}

/// Three rows a capital, one-pixel strokes.
pub fn compact() -> Font {
    Font::from_face("compact", &COMPACT)
}

/// One row tall: the characters themselves. Used when the terminal is too
/// short for block art, and selectable directly for a quieter display.
pub fn mini() -> Font {
    Font {
        name: "mini",
        height: 1,
        face: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY: &str = "\
cap 4
descent 2
gap 1
space 2
tracking 1

== o
..
##
##
..

== mark:acute
.#
";

    #[test]
    fn a_letter_without_its_descender_gets_blank_rows_for_one() {
        let face = Face::parse(TINY).unwrap();
        assert_eq!(face.glyphs[&'o'].rows.len(), 6);
    }

    #[test]
    fn an_accent_sits_gap_pixels_above_the_letter() {
        let face = Face::parse(TINY).unwrap();
        let b = face.draw('ó', &[]).unwrap();
        // `o`'s ink starts on row 1, so with a gap of one the acute is on -1.
        assert_eq!(b.ink_rows(), Some((-1, 2)));
        assert!(b.ink(1, -1) && !b.ink(0, -1));
    }

    #[test]
    fn a_wide_mark_widens_the_glyph_instead_of_being_cut() {
        let mut b = Bitmap::blank(2, 0, 2);
        let wide = Bitmap {
            width: 4,
            top: 0,
            rows: vec![vec![true; 4]],
        };
        b.stamp(&wide, -1, -2);
        assert_eq!(b.width, 4);
        assert_eq!(b.top, -2);
    }

    const CYRILLIC: &str = "АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдеёжзийклмнопрстуфхцчшщъыьэюя\
                            ІЇЄҐіїєґЎў";

    #[test]
    fn every_russian_letter_is_drawn_as_art_in_both_fonts() {
        for font in [block(), compact()] {
            for c in CYRILLIC.chars() {
                assert!(font.can_draw(c), "{}: no art for {c}", font.name);
                assert!(
                    matches!(font.draw(c, &[]), Glyph::Art(_)),
                    "{}: {c} fell back to plain text",
                    font.name
                );
            }
        }
    }

    #[test]
    fn a_decomposed_yo_draws_like_the_precomposed_one() {
        let font = block();
        assert_eq!(font.draw('ё', &[]), font.draw('е', &['\u{308}']));
        assert_eq!(font.draw('Й', &[]), font.draw('И', &['\u{306}']));
    }

    #[test]
    fn a_cyrillic_i_loses_its_dot_under_a_diaeresis() {
        let font = block();
        let (Glyph::Art(yi), Glyph::Art(i)) = (font.draw('ї', &[]), font.draw('і', &[])) else {
            panic!("expected art");
        };
        // A dotted `і` with a diaeresis on top would collide; the dotless stem
        // under two dots is wider than the plain `і`.
        assert!(yi.width > i.width);
        assert_ne!(yi, i);
    }

    #[test]
    fn malformed_files_are_rejected_with_a_reason() {
        assert!(Face::parse("cap 4\n").unwrap_err().contains("descent"));
        let ragged = format!("{TINY}\n== x\n##\n#\n##\n##\n");
        assert!(Face::parse(&ragged).unwrap_err().contains("width"));
        let short = format!("{TINY}\n== x\n##\n");
        assert!(Face::parse(&short).unwrap_err().contains("rows"));
    }
}
