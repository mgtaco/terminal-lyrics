//! One LRC parser.
//!
//! v1 shipped two parsers with different regexes: one accepted `[00:10]`, the
//! other silently dropped it, so lines that processed fine were invisible in the
//! player. There is one parser here and it handles the whole dialect:
//!
//! * `[mm:ss]`, `[mm:ss.xx]`, `[mm:ss.xxx]` — fractional seconds optional
//! * multiple timestamps on one line: `[00:10.00][01:20.00]same words`
//! * `[offset:+250]` / `[offset:-250]`, applied to every timestamp
//! * ID tags (`ti`, `ar`, `al`, `by`, `length`) kept as metadata, not lyrics
//! * enhanced (A2) word tags: `[00:12.00]<00:12.00>Hello <00:12.50>world`
//! * `#` comments and blank lines
//!
//! Timestamped lines with no text are kept: in a well-made LRC they mark
//! instrumental gaps, and dropping them (as v1 did) leaves the previous line
//! frozen on screen through the whole break.

use std::ops::Range;

/// A word with its own timestamp, from an enhanced-LRC source.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub start: f64,
    /// When the word stops being highlighted. A trailing `<mm:ss.xx>` tag sets
    /// this explicitly; otherwise finalisation fills in the next word's start.
    /// The difference matters when a source records a real gap between words —
    /// without it the highlight smears across the pause.
    pub end: f64,
    /// Char range of this word inside [`Line::text`], for highlighting.
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub start: f64,
    /// End of the line: the next line's start, or an estimate for the last one.
    pub end: f64,
    pub text: String,
    /// Real per-word timings. Empty when the source is line-level only —
    /// never synthesised here, so callers can tell the difference.
    pub words: Vec<Word>,
}

impl Line {
    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn char_len(&self) -> usize {
        self.text.chars().count()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub author: Option<String>,
    /// `[length:mm:ss]`, in seconds.
    pub length: Option<f64>,
    /// `[offset:ms]` as written; already applied to the timestamps.
    pub offset_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    pub meta: Meta,
}

impl Lyrics {
    /// True when at least one line carries real per-word timings.
    pub fn has_word_timings(&self) -> bool {
        self.lines.iter().any(|l| !l.words.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.is_blank())
    }

    /// Plain text, one line per lyric line — used for unsynced fallbacks.
    pub fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Parse `[mm:ss(.frac)?]` starting at `chars[i]`, returning the time and the
/// index just past the closing bracket. Minutes may exceed 99.
fn parse_timestamp(s: &[char], i: usize) -> Option<(f64, usize)> {
    if s.get(i) != Some(&'[') {
        return None;
    }
    let close = s[i..].iter().position(|&c| c == ']')? + i;
    let body: String = s[i + 1..close].iter().collect();
    let secs = parse_clock(&body)?;
    Some((secs, close + 1))
}

/// `mm:ss`, `mm:ss.xx`, `hh:mm:ss.xx` → seconds. Rejects anything else, which is
/// what keeps `[ti:Song]` from being read as a time. Shared with the TTML
/// converter, which uses the same clock syntax.
pub fn parse_clock(body: &str) -> Option<f64> {
    let mut parts = body.split(':');
    let a = parts.next()?;
    let b = parts.next()?;
    let c = parts.next();
    if parts.next().is_some() {
        return None;
    }

    let (h, m, rest) = match c {
        Some(c) => (a.parse::<u64>().ok()?, b.parse::<u64>().ok()?, c),
        None => (0, a.parse::<u64>().ok()?, b),
    };

    // Seconds may carry a fraction; both `.` and `,` appear in the wild.
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let normalised = rest.replace(',', ".");
    if !normalised
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.')
    {
        return None;
    }
    let s: f64 = normalised.parse().ok()?;
    if !s.is_finite() || s < 0.0 {
        return None;
    }
    Some(h as f64 * 3600.0 + m as f64 * 60.0 + s)
}

/// `<mm:ss.xx>` word tag at `chars[i]`.
fn parse_word_tag(s: &[char], i: usize) -> Option<(f64, usize)> {
    if s.get(i) != Some(&'<') {
        return None;
    }
    let close = s[i..].iter().position(|&c| c == '>')? + i;
    let body: String = s[i + 1..close].iter().collect();
    let secs = parse_clock(&body)?;
    Some((secs, close + 1))
}

/// An ID tag such as `[ar:Radiohead]`, returned as `(key, value)`.
fn parse_id_tag(line: &str) -> Option<(String, String)> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    let (key, value) = inner.split_once(':')?;
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some((key, value.trim().to_string()))
}

/// A line body split into text plus any word timings it carried.
struct Body {
    text: String,
    words: Vec<Word>,
}

fn parse_body(rest: &[char]) -> Body {
    let mut text = String::new();
    let mut words: Vec<Word> = Vec::new();
    let mut pending: Option<(f64, usize)> = None; // (start, char index in `text`)
    let mut chars = 0usize;
    let mut i = 0usize;

    while i < rest.len() {
        if let Some((t, next)) = parse_word_tag(rest, i) {
            if let Some((start, from)) = pending.take() {
                push_word(&mut words, &text, start, from, chars);
            }
            pending = Some((t, chars));
            i = next;
            continue;
        }
        text.push(rest[i]);
        chars += 1;
        i += 1;
    }

    if let Some((start, from)) = pending.take() {
        push_word(&mut words, &text, start, from, chars);
    }

    // Trailing whitespace would otherwise widen the last word's highlight.
    let trimmed = text.trim_end();
    if trimmed.len() != text.len() {
        let new_len = trimmed.chars().count();
        text.truncate(trimmed.len());
        for w in &mut words {
            w.range.end = w.range.end.min(new_len);
            w.range.start = w.range.start.min(w.range.end);
        }
    }

    Body { text, words }
}

/// Record a word spanning `from..to` (char indices), skipping the whitespace
/// that separates it from the next one so the highlight lands on the word.
///
/// A tag with no visible text behind it is an *end* tag: it closes the previous
/// word rather than opening a new one.
fn push_word(words: &mut Vec<Word>, text: &str, start: f64, from: usize, to: usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut end = to;
    while end > from && chars.get(end - 1).is_some_and(|c| c.is_whitespace()) {
        end -= 1;
    }
    if end <= from {
        if let Some(prev) = words.last_mut()
            && start >= prev.start
        {
            prev.end = start;
        }
        return;
    }
    words.push(Word {
        start,
        end: f64::INFINITY, // resolved in `finalise`
        range: from..end,
    });
}

/// Parse an LRC document. Never fails: malformed lines are skipped, because a
/// single bad line in a downloaded file should not cost the user their lyrics.
pub fn parse(input: &str) -> Lyrics {
    let mut meta = Meta::default();
    let mut lines: Vec<Line> = Vec::new();

    for raw in input.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let chars: Vec<char> = trimmed.chars().collect();

        // Collect every leading `[..]` that parses as a clock.
        let mut stamps: Vec<f64> = Vec::new();
        let mut i = 0usize;
        while let Some((t, next)) = parse_timestamp(&chars, i) {
            stamps.push(t);
            i = next;
        }

        if stamps.is_empty() {
            if let Some((key, value)) = parse_id_tag(trimmed) {
                match key.as_str() {
                    "ti" => meta.title = Some(value),
                    "ar" => meta.artist = Some(value),
                    "al" => meta.album = Some(value),
                    "by" => meta.author = Some(value),
                    "length" => meta.length = parse_clock(&value),
                    "offset" => {
                        if let Ok(ms) = value.trim_start_matches('+').parse::<i64>() {
                            meta.offset_ms = ms;
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }

        let body = parse_body(&chars[i..]);
        for start in stamps {
            lines.push(Line {
                start,
                end: f64::INFINITY,
                text: body.text.clone(),
                words: body.words.clone(),
            });
        }
    }

    finalise(lines, meta)
}

/// Sort, apply `[offset:]`, and fill in every `end` so lookup never has to
/// reason about neighbours.
fn finalise(mut lines: Vec<Line>, meta: Meta) -> Lyrics {
    // Multi-timestamp lines arrive out of order by construction.
    lines.sort_by(|a, b| a.start.total_cmp(&b.start));

    if meta.offset_ms != 0 {
        let shift = meta.offset_ms as f64 / 1000.0;
        for line in &mut lines {
            line.start += shift;
            for w in &mut line.words {
                w.start += shift;
            }
        }
    }

    for idx in 0..lines.len() {
        let next_start = lines.get(idx + 1).map(|l| l.start);
        let line = &mut lines[idx];
        // The last line has no successor; give it a bounded life so the screen
        // clears at the end of the song instead of holding the final phrase.
        line.end = next_start.unwrap_or_else(|| {
            let est = estimate_duration(line.char_len());
            line.start + est
        });
        if line.end < line.start {
            line.end = line.start;
        }

        let line_end = line.end;
        let count = line.words.len();
        for w in 0..count {
            // An end tag already set this; only fill in the ones still open.
            if line.words[w].end.is_finite() {
                continue;
            }
            let end = if w + 1 < count {
                line.words[w + 1].start
            } else {
                line_end
            };
            let w = &mut line.words[w];
            w.end = end.max(w.start);
        }
    }

    Lyrics { lines, meta }
}

/// Rough reading time for the final line only, so it does not linger forever.
fn estimate_duration(chars: usize) -> f64 {
    (chars as f64 / 12.0).clamp(2.0, 8.0)
}
