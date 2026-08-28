//! Query cleanup for *fallback* lookups only.
//!
//! The first attempt always uses the player's metadata exactly as given —
//! LRCLIB indexes real release titles, so "cleaning" up front loses matches.
//! Only when that misses do we strip the decorations that streaming services
//! append. Nothing here ever changes what is displayed.

/// Suffixes after a dash: `Song - Remastered 2011`, `Song - Live`.
const DASH_SUFFIXES: &[&str] = &[
    "remaster",
    "remastered",
    "live",
    "radio edit",
    "single version",
    "album version",
    "extended",
    "extended version",
    "deluxe",
    "bonus track",
    "mono",
    "stereo",
    "explicit",
    "clean",
    "instrumental",
    "acoustic",
    "demo",
    "edit",
    "version",
    "anniversary edition",
];

/// Parenthesised decorations: `(feat. X)`, `(Remastered)`, `[Explicit]`.
const BRACKET_MARKERS: &[&str] = &[
    "feat.", "feat ", "ft.", "ft ", "with ", "remaster", "live", "explicit", "bonus", "deluxe",
    "version", "edit", "mono", "stereo", "remixed",
];

/// Does the text after a `-` look like a decoration rather than part of a title?
fn is_decoration(tail: &str) -> bool {
    let t = tail.trim().to_lowercase();
    if t.is_empty() {
        return false;
    }
    DASH_SUFFIXES.iter().any(|s| {
        // Matches "Remastered", "Remastered 2011", "2011 Remaster".
        t == *s || t.starts_with(&format!("{s} ")) || t.ends_with(&format!(" {s}"))
    })
}

fn strip_dash_suffix(title: &str) -> String {
    let mut out = title.to_string();
    // Repeat: `Song - Live - Remastered 2011`.
    while let Some(idx) = out.rfind(" - ") {
        let (head, tail) = out.split_at(idx);
        if is_decoration(&tail[3..]) {
            out = head.to_string();
        } else {
            break;
        }
    }
    out
}

fn strip_brackets(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut depth = 0usize;
    let mut buf = String::new();
    let mut opener = '(';

    for c in title.chars() {
        match c {
            '(' | '[' if depth == 0 => {
                depth = 1;
                opener = c;
                buf.clear();
            }
            ')' | ']' if depth == 1 => {
                depth = 0;
                let lower = buf.to_lowercase();
                let decorative = BRACKET_MARKERS.iter().any(|m| lower.starts_with(m));
                if !decorative {
                    // Keep it: `(Don't Fear) The Reaper` is part of the title.
                    let close = if opener == '(' { ')' } else { ']' };
                    out.push(opener);
                    out.push_str(&buf);
                    out.push(close);
                }
            }
            _ if depth == 1 => buf.push(c),
            _ => out.push(c),
        }
    }
    // Unbalanced bracket: keep what we swallowed rather than dropping words.
    if depth == 1 {
        out.push(opener);
        out.push_str(&buf);
    }
    collapse_spaces(&out)
}

fn collapse_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A relaxed title for the second attempt. Returns `None` when it would be
/// identical to the input, so callers can skip a pointless request.
pub fn relax_title(title: &str) -> Option<String> {
    let out = collapse_spaces(&strip_brackets(&strip_dash_suffix(title)));
    if out.is_empty() || out.eq_ignore_ascii_case(title.trim()) {
        None
    } else {
        Some(out)
    }
}

/// Artists arrive joined in all sorts of ways; LRCLIB indexes the primary one.
pub fn primary_artist(artist: &str) -> Option<String> {
    for sep in [
        " feat. ", " feat ", " ft. ", " ft ", " & ", ", ", " x ", " with ",
    ] {
        if let Some((head, _)) = artist.split_once(sep) {
            let head = head.trim();
            if !head.is_empty() && !head.eq_ignore_ascii_case(artist.trim()) {
                return Some(head.to_string());
            }
        }
    }
    None
}
