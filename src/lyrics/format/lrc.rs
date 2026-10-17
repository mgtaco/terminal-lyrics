//! Plain, synced and enhanced (word-synced) LRC.

use std::sync::LazyLock;

use regex::Regex;

use super::{MAX_LINES, MAX_SOURCE_BYTES, MAX_WORDS, decode};
use crate::lyrics::error::{Error, Result};
use crate::lyrics::model::{Line, Lyrics, Seconds, SourceFormat, Word, sort_lines, trim_words};

/// `<mm:ss.xx>`. ASCII digits only: Rust's `\d` would take any script's.
static ENHANCED_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<[0-9]+:[0-9]+(?::[0-9]+)?(?:[.,][0-9]+)?>").expect("static regex")
});

pub fn parse(data: &[u8]) -> Result<Lyrics> {
    let text = decode(data)?;

    let mut lines: Vec<Line> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    let mut offset: Seconds = 0.0;
    let mut expanded_bytes = 0usize;
    let mut expanded_words = 0usize;

    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        let (stamps, rest) = split_timestamps(raw);

        if stamps.is_empty() {
            if let Some((name, value)) = meta_tag(raw) {
                if name == "offset" {
                    offset = parse_offset(&value);
                }
                continue;
            }
            let body = strip_enhanced(raw);
            let body = body.trim();
            if !body.is_empty() {
                plain.push(body.to_string());
            }
            if plain.len() > MAX_LINES {
                return Err(Error::TooLarge);
            }
            continue;
        }

        let (body, words) = split_words(rest, stamps[0]);
        if !body.is_empty() {
            plain.push(body.clone());
        }
        expanded_bytes += body.len() * stamps.len();
        expanded_words += words.len() * stamps.len();
        if lines.len() + stamps.len() > MAX_LINES
            || plain.len() > MAX_LINES
            || expanded_bytes > MAX_SOURCE_BYTES
            || expanded_words > MAX_WORDS
        {
            return Err(Error::TooLarge);
        }
        for &at in &stamps {
            let mut line = Line {
                start: at,
                text: body.clone(),
                ..Default::default()
            };
            for word in &words {
                let mut shifted = Word {
                    start: at + (word.start - stamps[0]),
                    end: 0.0,
                    text: word.text.clone(),
                };
                if word.end > word.start {
                    shifted.end = at + (word.end - stamps[0]);
                }
                if !shifted.start.is_finite() || !shifted.end.is_finite() {
                    return Err(Error::invalid("lyric timestamp overflow"));
                }
                line.words.push(shifted);
            }
            lines.push(line);
        }
    }

    for line in &mut lines {
        line.start = shift(line.start, offset);
        for word in &mut line.words {
            word.start = shift(word.start, offset);
            if word.end > 0.0 {
                word.end = shift(word.end, offset);
            }
        }
    }
    sort_lines(&mut lines);

    Ok(Lyrics {
        synced: lines,
        plain: plain.join("\n"),
        source: SourceFormat::Lrc,
        ..Default::default()
    })
}

/// `[offset:]` moves every stamp earlier by that many milliseconds.
fn shift(value: Seconds, offset: Seconds) -> Seconds {
    let shifted = value - offset;
    if !shifted.is_finite() {
        return value;
    }
    shifted.max(0.0)
}

fn split_timestamps(line: &str) -> (Vec<Seconds>, &str) {
    let mut stamps = Vec::new();
    let mut rest = line;
    loop {
        let trimmed = rest.trim_start_matches([' ', '\t']);
        if !trimmed.starts_with('[') {
            break;
        }
        let Some(end) = trimmed.find(']') else { break };
        let Some(at) = parse_stamp(&trimmed[1..end]) else {
            break;
        };
        stamps.push(at);
        rest = &trimmed[end + 1..];
    }
    if stamps.is_empty() {
        return (stamps, line);
    }
    (stamps, rest)
}

/// Go's `strconv.ParseFloat`, near enough: Rust also takes `inf` and `nan`,
/// which are rejected right after anyway.
fn parse_float(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `mm:ss`, `mm:ss.xx` or `hh:mm:ss.xx`; only the last part may carry a
/// fraction, written with `.` or `,`.
pub(crate) fn parse_stamp(s: &str) -> Option<Seconds> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut total: Seconds = 0.0;
    for (i, part) in parts.iter().enumerate() {
        let part = if i == parts.len() - 1 {
            part.replacen(',', ".", 1)
        } else if part.contains(['.', ',']) {
            return None;
        } else {
            part.to_string()
        };
        let value = parse_float(&part).filter(|v| *v >= 0.0)?;
        total = total * 60.0 + value;
    }
    total.is_finite().then_some(total)
}

fn meta_tag(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    let (name, value) = inner.split_once(':')?;
    Some((name.trim().to_lowercase(), value.trim().to_string()))
}

fn parse_offset(value: &str) -> Seconds {
    let value = value.trim();
    let value = value.strip_prefix('+').unwrap_or(value);
    match parse_float(value) {
        Some(ms) => ms / 1000.0,
        None => 0.0,
    }
}

fn strip_enhanced(s: &str) -> String {
    if !s.contains('<') {
        return s.to_string();
    }
    ENHANCED_TAG.replace_all(s, "").into_owned()
}

/// The line's text, and its words when every tag reads and none goes backwards.
fn split_words(text: &str, start: Seconds) -> (String, Vec<Word>) {
    let tags: Vec<(usize, usize)> = ENHANCED_TAG
        .find_iter(text)
        .map(|m| (m.start(), m.end()))
        .collect();
    if tags.is_empty() {
        return (text.trim().to_string(), Vec::new());
    }
    let body = strip_enhanced(text).trim().to_string();
    let mut words = Vec::new();
    let mut previous = 0usize;
    let mut at = start;
    for (from, to) in tags {
        let next = match parse_stamp(&text[from + 1..to - 1]) {
            Some(next) if next >= at => next,
            _ => return (body, Vec::new()),
        };
        if from > previous {
            words.push(Word {
                start: at,
                end: next,
                text: text[previous..from].to_string(),
            });
        }
        at = next;
        previous = to;
    }
    if previous < text.len() {
        words.push(Word {
            start: at,
            end: 0.0,
            text: text[previous..].to_string(),
        });
    }
    (body, trim_words(words))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enhanced_words_keep_their_spaces() {
        let parsed = parse(b"[00:01.00]<00:01.00>Hello <00:01.50>world<00:02.00>").unwrap();
        let line = &parsed.synced[0];
        assert_eq!(line.text, "Hello world");
        let texts: Vec<_> = line.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["Hello ", "world"]);
        assert_eq!(line.words[1].end, 2.0);
    }

    #[test]
    fn offset_moves_stamps_earlier() {
        let parsed = parse(b"[offset:+500]\n[00:02.00]late").unwrap();
        assert_eq!(parsed.synced[0].start, 1.5);
    }

    #[test]
    fn repeated_stamps_expand_and_sort() {
        let parsed = parse(b"[00:10.00][00:01.00]again\n[00:05.00]mid").unwrap();
        let starts: Vec<_> = parsed.synced.iter().map(|l| l.start).collect();
        assert_eq!(starts, [1.0, 5.0, 10.0]);
    }

    #[test]
    fn backwards_word_tags_drop_the_words_only() {
        let parsed = parse(b"[00:05.00]<00:05.00>a <00:04.00>b").unwrap();
        assert_eq!(parsed.synced[0].text, "a b");
        assert!(parsed.synced[0].words.is_empty());
    }

    #[test]
    fn untimed_text_is_plain() {
        let parsed = parse(b"[ar:Someone]\nfirst\nsecond").unwrap();
        assert!(parsed.synced.is_empty());
        assert_eq!(parsed.plain, "first\nsecond");
    }
}
