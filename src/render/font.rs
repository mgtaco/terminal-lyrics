//! Block fonts.
//!
//! The look is carried over from v1 — five-row solid block capitals — because
//! that part was the good bit. What is new is that every glyph knows its width,
//! so the layout can wrap instead of truncating.

use std::collections::HashMap;

use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone)]
pub struct Font {
    pub name: &'static str,
    pub height: usize,
    glyphs: HashMap<char, Vec<&'static str>>,
    space: Vec<&'static str>,
    /// When set, characters render as themselves instead of as block art.
    /// This is the honest fallback for a terminal too short for a real font.
    plain: bool,
}

impl Font {
    fn build(name: &'static str, height: usize, table: &[(char, &'static [&'static str])]) -> Self {
        let mut glyphs = HashMap::new();
        for (c, rows) in table {
            debug_assert_eq!(rows.len(), height, "glyph {c:?} in {name} has the wrong height");
            glyphs.insert(*c, rows.to_vec());
        }
        let space = glyphs
            .get(&' ')
            .cloned()
            .unwrap_or_else(|| vec!["  "; height]);
        Self {
            name,
            height,
            glyphs,
            space,
            plain: false,
        }
    }

    /// Rows for one character. Unknown characters render as a space, which
    /// keeps a stray emoji in a track title from breaking the layout.
    pub fn glyph(&self, c: char) -> Vec<String> {
        if self.plain {
            return vec![c.to_string()];
        }
        let upper = c.to_ascii_uppercase();
        self.glyphs
            .get(&upper)
            .unwrap_or(&self.space)
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    pub fn width_of(&self, c: char) -> usize {
        if self.plain {
            return UnicodeWidthChar::width(c).unwrap_or(1);
        }
        let upper = c.to_ascii_uppercase();
        self.glyphs
            .get(&upper)
            .unwrap_or(&self.space)
            .first()
            .map(|r| r.chars().count())
            .unwrap_or(0)
    }

    /// Columns inserted between adjacent glyphs. Block art needs the gap;
    /// plain text already has spaces in it.
    pub fn tracking(&self) -> usize {
        if self.plain { 0 } else { 1 }
    }
}

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

macro_rules! font {
    ($name:literal, $height:literal, { $($ch:literal => [$($row:literal),* $(,)?]),* $(,)? }) => {
        Font::build($name, $height, &[ $( ($ch, &[$($row),*]) ),* ])
    };
}

pub fn block() -> Font {
    font!("block", 5, {
        'A' => ["  ███  ", " ██ ██ ", "███████", "██   ██", "██   ██"],
        'B' => ["██████ ", "██   ██", "██████ ", "██   ██", "██████ "],
        'C' => [" █████ ", "██   ██", "██     ", "██   ██", " █████ "],
        'D' => ["██████ ", "██   ██", "██   ██", "██   ██", "██████ "],
        'E' => ["███████", "██     ", "█████  ", "██     ", "███████"],
        'F' => ["███████", "██     ", "█████  ", "██     ", "██     "],
        'G' => [" █████ ", "██     ", "██  ███", "██   ██", " █████ "],
        'H' => ["██   ██", "██   ██", "███████", "██   ██", "██   ██"],
        'I' => ["███", " ██", " ██", " ██", "███"],
        'J' => ["     ██", "     ██", "     ██", "██   ██", " █████ "],
        'K' => ["██   ██", "██  ██ ", "█████  ", "██  ██ ", "██   ██"],
        'L' => ["██     ", "██     ", "██     ", "██     ", "███████"],
        'M' => ["██   ██", "███ ███", "███████", "██ █ ██", "██   ██"],
        'N' => ["██   ██", "███  ██", "████ ██", "██ ████", "██   ██"],
        'O' => [" █████ ", "██   ██", "██   ██", "██   ██", " █████ "],
        'P' => ["██████ ", "██   ██", "██████ ", "██     ", "██     "],
        'Q' => [" █████ ", "██   ██", "██   ██", "██  ███", " ██████"],
        'R' => ["██████ ", "██   ██", "██████ ", "██  ██ ", "██   ██"],
        'S' => [" █████ ", "██     ", " █████ ", "     ██", " █████ "],
        'T' => ["███████", "  ██   ", "  ██   ", "  ██   ", "  ██   "],
        'U' => ["██   ██", "██   ██", "██   ██", "██   ██", " █████ "],
        'V' => ["██   ██", "██   ██", "██   ██", " ██ ██ ", "  ███  "],
        'W' => ["██   ██", "██   ██", "██ █ ██", "███████", "███ ███"],
        'X' => ["██   ██", " ██ ██ ", "  ███  ", " ██ ██ ", "██   ██"],
        'Y' => ["██   ██", " ██ ██ ", "  ███  ", "  ██   ", "  ██   "],
        'Z' => ["███████", "    ██ ", "  ███  ", " ██    ", "███████"],
        '0' => [" █████ ", "██   ██", "██   ██", "██   ██", " █████ "],
        '1' => ["  ██   ", " ███   ", "  ██   ", "  ██   ", "███████"],
        '2' => [" █████ ", "██   ██", "   ███ ", " ██    ", "███████"],
        '3' => [" █████ ", "██   ██", "  ████ ", "██   ██", " █████ "],
        '4' => ["██   ██", "██   ██", "███████", "     ██", "     ██"],
        '5' => ["███████", "██     ", "██████ ", "     ██", "██████ "],
        '6' => [" █████ ", "██     ", "██████ ", "██   ██", " █████ "],
        '7' => ["███████", "     ██", "    ██ ", "   ██  ", "  ██   "],
        '8' => [" █████ ", "██   ██", " █████ ", "██   ██", " █████ "],
        '9' => [" █████ ", "██   ██", " ██████", "     ██", " █████ "],
        ' ' => ["    ", "    ", "    ", "    ", "    "],
        '\'' => ["██", "██", "  ", "  ", "  "],
        '"' => ["██ ██", "██ ██", "     ", "     ", "     "],
        ',' => ["  ", "  ", "  ", "██", "█ "],
        '.' => ["  ", "  ", "  ", "  ", "██"],
        '!' => ["██", "██", "██", "  ", "██"],
        '?' => [" ███ ", "█   █", "   █ ", "     ", "  █  "],
        '-' => ["      ", "      ", "██████", "      ", "      "],
        '(' => [" ██", "██ ", "██ ", "██ ", " ██"],
        ')' => ["██ ", " ██", " ██", " ██", "██ "],
        '[' => ["███", "██ ", "██ ", "██ ", "███"],
        ']' => ["███", " ██", " ██", " ██", "███"],
        ':' => ["  ", "██", "  ", "██", "  "],
        ';' => ["  ", "██", "  ", "██", "█ "],
        '/' => ["    ██", "   ██ ", "  ██  ", " ██   ", "██    "],
        '\\' => ["██    ", " ██   ", "  ██  ", "   ██ ", "    ██"],
        '&' => [" ███  ", "█   █ ", " ███  ", "█ █ █ ", " ███ █"],
        '#' => [" █ █ ", "█████", " █ █ ", "█████", " █ █ "],
        '*' => ["█ █ █", " ███ ", "█████", " ███ ", "█ █ █"],
        '+' => ["  ██  ", "  ██  ", "██████", "  ██  ", "  ██  "],
        '=' => ["      ", "██████", "      ", "██████", "      "],
        '<' => ["   ██ ", "  ██  ", " ██   ", "  ██  ", "   ██ "],
        '>' => [" ██   ", "  ██  ", "   ██ ", "  ██  ", " ██   "],
        '@' => [" ████ ", "█    █", "█ ██ █", "█ ██ █", " ████ "],
        '$' => [" ████ ", "██ █  ", " ████ ", "  █ ██", " ████ "],
        '%' => ["██  █ ", "██ ██ ", "  ██  ", " ██ ██", " █  ██"],
    })
}

pub fn compact() -> Font {
    font!("compact", 3, {
        'A' => ["▄▀▄", "█▀█", "▀ ▀"],
        'B' => ["█▀▄", "█▀▄", "▀▀ "],
        'C' => ["▄▀▀", "█  ", "▀▀▀"],
        'D' => ["█▀▄", "█ █", "▀▀ "],
        'E' => ["█▀▀", "█▀ ", "▀▀▀"],
        'F' => ["█▀▀", "█▀ ", "▀  "],
        'G' => ["▄▀▀", "█ ▀", "▀▀▀"],
        'H' => ["█ █", "█▀█", "▀ ▀"],
        'I' => ["█", "█", "▀"],
        'J' => ["  █", "  █", "▀▀ "],
        'K' => ["█ █", "█▀ ", "▀ ▀"],
        'L' => ["█  ", "█  ", "▀▀▀"],
        'M' => ["█▄▀▄█", "█ ▀ █", "▀   ▀"],
        'N' => ["█▄ █", "█ ▀█", "▀  ▀"],
        'O' => ["▄▀▀▄", "█  █", "▀▀▀ "],
        'P' => ["█▀▄", "█▀ ", "▀  "],
        'Q' => ["▄▀▀▄", "█ ▀█", "▀▀ ▀"],
        'R' => ["█▀▄", "█▀▄", "▀ ▀"],
        'S' => ["▄▀▀", " ▀▄", "▀▀ "],
        'T' => ["▀█▀", " █ ", " ▀ "],
        'U' => ["█ █", "█ █", "▀▀▀"],
        'V' => ["█ █", "█ █", " ▀ "],
        'W' => ["█   █", "█ ▄ █", "▀▀▀▀▀"],
        'X' => ["█ █", " ▀ ", "▀ ▀"],
        'Y' => ["█ █", " ▀ ", " ▀ "],
        'Z' => ["▀▀█", " ▄▀", "█▀▀"],
        '0' => ["▄▀▄", "█ █", "▀▄▀"],
        '1' => ["▄█ ", " █ ", "▀▀▀"],
        '2' => ["▀▀▄", "▄▀ ", "▀▀▀"],
        '3' => ["▀▀▄", " ▀▄", "▀▀ "],
        '4' => ["█ █", "▀▀█", "  ▀"],
        '5' => ["█▀▀", "▀▀▄", "▀▀ "],
        '6' => ["▄▀▀", "█▀▄", "▀▀ "],
        '7' => ["▀▀█", " ▄▀", " █ "],
        '8' => ["▄▀▄", "▄▀▄", "▀▀ "],
        '9' => ["▄▀▄", "▀▀█", "▀▀ "],
        ' ' => ["  ", "  ", "  "],
        '\'' => ["█", " ", " "],
        '"' => ["█ █", "   ", "   "],
        ',' => [" ", " ", "▄"],
        '.' => [" ", " ", "▄"],
        '!' => ["█", "█", "▄"],
        '?' => ["▀▄", " ▀", " ▄"],
        '-' => ["   ", "▀▀▀", "   "],
        '(' => ["▄", "█", "▀"],
        ')' => ["▄", "█", "▀"],
        ':' => [" ", "▄", "▄"],
        ';' => [" ", "▄", "▄"],
        '/' => ["  ▄", " ▀ ", "▄  "],
        '&' => ["▄▀▄", "▄▀▄", "▀ ▀"],
    })
}

/// One row tall: the characters themselves. Used when the terminal is too
/// short for block art, and selectable directly for a quieter display.
pub fn mini() -> Font {
    Font {
        name: "mini",
        height: 1,
        glyphs: HashMap::new(),
        space: vec![" "],
        plain: true,
    }
}
