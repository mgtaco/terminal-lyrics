//! The song name in the status row, and how a new name types over the old.

use std::time::Duration;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// How long the typing takes over each column.
pub const PER_CHAR: Duration = Duration::from_millis(30);

/// `to` typing over `from`, a column at a time from the left, `elapsed` in.
/// Both are right-aligned, as in the corner, so the old name stays where it is
/// and a longer new one starts typing ahead of it.
pub fn typewriter(from: &str, to: &str, elapsed: Duration) -> String {
    let (old, new) = (cells(from), cells(to));
    let width = old.len().max(new.len());
    let cursor = (elapsed.as_millis() / PER_CHAR.as_millis()) as usize;
    if cursor >= width {
        return to.to_string();
    }
    let (old, new) = (pad(old, width), pad(new, width));
    let mut out = String::with_capacity(to.len().max(from.len()));
    let mut x = 0;
    while x < width {
        let grid = if x < cursor { &new } else { &old };
        match &grid[x] {
            Cell::Head(s, w) => {
                out.push_str(s);
                x += w;
            }
            // Half of a wide character whose other half was typed over.
            Cell::Blank | Cell::Tail => {
                out.push(' ');
                x += 1;
            }
        }
    }
    out
}

#[derive(Clone)]
enum Cell {
    Blank,
    /// A character, with any marks on it, and how many columns it takes.
    Head(String, usize),
    /// The second column of a wide character.
    Tail,
}

fn cells(s: &str) -> Vec<Cell> {
    let mut out = Vec::new();
    for c in s.chars() {
        match c.width().unwrap_or(0) {
            0 => {
                if let Some(Cell::Head(head, _)) =
                    out.iter_mut().rev().find(|c| matches!(c, Cell::Head(..)))
                {
                    head.push(c);
                }
            }
            w => {
                out.push(match c {
                    ' ' => Cell::Blank,
                    c => Cell::Head(c.to_string(), w),
                });
                out.extend(std::iter::repeat_n(Cell::Tail, w - 1));
            }
        }
    }
    out
}

fn pad(cells: Vec<Cell>, width: usize) -> Vec<Cell> {
    let mut out = vec![Cell::Blank; width - cells.len()];
    out.extend(cells);
    out
}

/// `left` from the left edge and `right` against the right one, a column in
/// from each. When both do not fit, `right` gives way: `left` is the source or
/// the message after a keypress.
pub fn status_row(left: &str, right: &str, width: usize) -> String {
    let left = match left.is_empty() {
        true => String::new(),
        false => format!(" {left}"),
    };
    let used = left.width();
    // Two columns between the two, one after `right`.
    let room = width.saturating_sub(used + 3);
    if right.is_empty() || room == 0 {
        return left;
    }
    let right = truncate(right, room);
    let pad = width - used - right.width() - 1;
    format!("{left}{}{right} ", " ".repeat(pad))
}

fn truncate(s: &str, room: usize) -> String {
    if s.width() <= room {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > room {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}
