//! Apple-style word-timed TTML, converted to enhanced (A2) LRC.
//!
//! The conversion is deliberate rather than lazy. Parsing TTML into `Lyrics`
//! directly would mean the cache had a second format to store and re-parse; by
//! emitting A2 instead, one parser and one cache format serve every source, and
//! the text we cache is exactly the text we parsed.
//!
//! Word ends are preserved with A2 end tags, so a real pause between words
//! stays a pause instead of the highlight sliding through it:
//!
//! ```text
//! <span begin="00:22.516" end="00:23.208">Flashing</span>
//! → [00:22.516]<00:22.516>Flashing<00:23.208> <00:23.846>Lights<00:24.698>
//! ```

use anyhow::{Result, anyhow};
use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::lrc;

/// A TTML time expression.
///
/// The spec allows a clock time (`00:04.658`, `01:02:03.4`) or an offset with a
/// metric suffix (`4.658s`, `250ms`). Real files also write a bare number,
/// meaning seconds — and AMLL switches between forms *within one file*, writing
/// `4.658` below a minute and `1:04.579` above it. Handling only the colon form
/// silently drops every line in the first minute.
pub fn parse_time(raw: &str) -> Option<f64> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if t.contains(':') {
        return lrc::parse_clock(t);
    }

    // Offset time: a number with an optional metric suffix.
    let (number, metric) = match t.find(|c: char| c.is_ascii_alphabetic()) {
        Some(i) => (&t[..i], &t[i..]),
        None => (t, ""),
    };
    let value: f64 = number.trim().replace(',', ".").parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let seconds = match metric.trim() {
        "" | "s" => value,
        "ms" => value / 1000.0,
        "m" => value * 60.0,
        "h" => value * 3600.0,
        // Frames and ticks need the document's frame rate, which these files do
        // not carry; treating them as seconds would be a silent lie.
        _ => return None,
    };
    Some(seconds)
}

/// Spans that are not part of the sung line.
fn is_skippable_role(role: &str) -> bool {
    // Translations and romanisations are separate text; background vocals are a
    // second voice that would interleave confusingly with the main line.
    role.starts_with("x-translation")
        || role.starts_with("x-roman")
        || role.starts_with("x-bg")
}

/// The `dur` attribute of `<body>` (or `<tt>`): the length of the recording the
/// lyrics were timed against.
///
/// Apple writes the real track length here — Radiohead's *Creep* comes back as
/// `dur="3:58.640"`, which is its Spotify duration to the millisecond. That
/// makes it good enough to reject a document timed against a different edit,
/// which is the one way a "matching" set of lyrics can be wrong from the first
/// line to the last.
pub fn document_duration(xml: &str) -> Option<f64> {
    let mut reader = Reader::from_str(xml);
    loop {
        let e = match reader.read_event() {
            Ok(Event::Start(e)) => e,
            Ok(Event::Empty(e)) => e,
            Ok(Event::Eof) | Err(_) => return None,
            _ => continue,
        };
        match local_name(e.name().as_ref()) {
            "tt" | "body" => {
                if let Some(d) = attr(&e, "dur").as_deref().and_then(parse_time) {
                    return Some(d);
                }
            }
            // Into the lyrics themselves: there is no duration to find.
            "div" | "p" => return None,
            _ => {}
        }
    }
}

/// `mm:ss.SSS`, the timestamp form every LRC line here is written in. Shared
/// with the providers that build A2 lines without going through TTML.
pub(crate) fn format_time(secs: f64) -> String {
    let secs = secs.max(0.0);
    let minutes = (secs / 60.0).floor() as u64;
    let rest = secs - minutes as f64 * 60.0;
    format!("{minutes:02}:{rest:06.3}")
}

/// One `<p>` being accumulated.
#[derive(Default)]
struct Line {
    begin: Option<f64>,
    /// A2 body: word tags interleaved with the literal text between them.
    body: String,
    words: usize,
}

/// Convert a TTML document into an enhanced-LRC string.
pub fn to_enhanced_lrc(xml: &str) -> Result<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut line: Option<Line> = None;
    // Depth of nested spans we are ignoring (a translation, or background vocals).
    let mut skip_depth = 0usize;
    let mut span_stack: Vec<Option<f64>> = Vec::new();
    let mut saw_body = false;
    // Counted so a `<p>` we cannot place in time is reported rather than
    // skipped. Partial lyrics are worse than none: they look like the song has
    // no first verse, and the fallback to LRCLIB never gets a chance to run.
    let mut unparsable_lines = 0usize;

    loop {
        match reader.read_event() {
            Err(e) => return Err(anyhow!("malformed TTML at byte {}: {e}", reader.buffer_position())),
            Ok(Event::Eof) => break,

            // `Empty` is a self-closing tag: it opens and closes at once, so it
            // must not be pushed onto the span stack.
            Ok(Event::Empty(e)) => {
                if local_name(e.name().as_ref()) == "br"
                    && skip_depth == 0
                    && let Some(l) = line.as_mut()
                {
                    l.body.push(' ');
                }
            }

            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref()).to_string();
                match name.as_str() {
                    "body" => saw_body = true,
                    "p" => {
                        let raw = attr(&e, "begin");
                        let begin = raw.as_deref().and_then(parse_time);
                        if begin.is_none() {
                            unparsable_lines += 1;
                        }
                        line = Some(Line {
                            begin,
                            ..Default::default()
                        });
                    }
                    "span" => {
                        let role = attr(&e, "role").unwrap_or_default();
                        if skip_depth > 0 || is_skippable_role(&role) {
                            skip_depth += 1;
                            span_stack.push(None);
                            continue;
                        }
                        let begin = attr(&e, "begin").and_then(|v| parse_time(&v));
                        let end = attr(&e, "end").and_then(|v| parse_time(&v));
                        if let (Some(l), Some(b)) = (line.as_mut(), begin) {
                            l.body.push_str(&format!("<{}>", format_time(b)));
                            l.words += 1;
                        }
                        span_stack.push(end);
                    }
                    _ => {}
                }
            }

            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                "span" => {
                    let end = span_stack.pop().flatten();
                    if skip_depth > 0 {
                        skip_depth -= 1;
                        continue;
                    }
                    // Close the word so a gap before the next one is honoured.
                    if let (Some(l), Some(end)) = (line.as_mut(), end) {
                        l.body.push_str(&format!("<{}>", format_time(end)));
                    }
                }
                "p" => {
                    if let Some(l) = line.take()
                        && let Some(begin) = l.begin
                    {
                        let body = l.body.trim();
                        out.push_str(&format!("[{}]{}\n", format_time(begin), body));
                    }
                }
                _ => {}
            },

            Ok(Event::Text(t)) => {
                if skip_depth > 0 {
                    continue;
                }
                if let Some(l) = line.as_mut() {
                    push_text(&mut l.body, t.xml10_content().as_ref());
                }
            }

            // Entities arrive as their own events rather than inside the text,
            // so `don&apos;t` would otherwise come through as `dont`.
            Ok(Event::GeneralRef(r)) => {
                if skip_depth > 0 {
                    continue;
                }
                if let Some(l) = line.as_mut()
                    && let Some(resolved) = resolve_entity(r.xml10_content().as_ref())
                {
                    l.body.push_str(&resolved);
                }
            }

            _ => {}
        }
    }

    if !saw_body {
        return Err(anyhow!("not a TTML document: no <body> element"));
    }
    if out.trim().is_empty() {
        return Err(anyhow!("TTML document contained no timed lines"));
    }
    if unparsable_lines > 0 {
        return Err(anyhow!(
            "{unparsable_lines} line(s) had a time this parser does not understand; \
             refusing to show partial lyrics"
        ));
    }
    Ok(out)
}

/// Append text, collapsing whitespace runs to a single space.
///
/// TTML in this database is emitted on one line, but pretty-printed files put
/// newlines and indentation between spans. Those are word separators, not part
/// of the lyric — and a raw newline would split the A2 line in two, silently
/// losing every word after the break.
fn push_text(body: &mut String, text: &str) {
    for (i, part) in text.split_whitespace().enumerate() {
        let needs_gap = i > 0 || (text.starts_with(char::is_whitespace) && !body.is_empty());
        if needs_gap && !body.ends_with(' ') {
            body.push(' ');
        }
        body.push_str(part);
    }
    // Trailing whitespace is a separator too.
    if text.ends_with(char::is_whitespace) && !body.is_empty() && !body.ends_with(' ') {
        body.push(' ');
    }
}

/// The five predefined XML entities, plus numeric character references.
fn resolve_entity(name: &str) -> Option<String> {
    match name {
        "amp" => return Some("&".into()),
        "lt" => return Some("<".into()),
        "gt" => return Some(">".into()),
        "quot" => return Some("\"".into()),
        "apos" => return Some("'".into()),
        _ => {}
    }
    let digits = name.strip_prefix('#')?;
    let code = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => digits.parse().ok()?,
    };
    char::from_u32(code).map(String::from)
}

/// TTML is namespaced (`ttm:role`, `tts:...`); match on the local part.
fn local_name(raw: &str) -> &str {
    match raw.rsplit_once(':') {
        Some((_, local)) => local,
        None => raw,
    }
}

fn attr(e: &quick_xml::events::BytesStart<'_>, want: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (local_name(a.key.as_ref()) == want)
            .then(|| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.to_string()))
            .flatten()
    })
}
