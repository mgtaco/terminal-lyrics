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

/// Spans that are not part of the sung lyric at all.
fn is_skippable_role(role: &str) -> bool {
    // Translations and romanisations are a different text for the same words.
    // Background vocals used to be dropped here too; they are a second voice,
    // and are now captured instead — see `SpanKind::Background`.
    role.starts_with("x-translation") || role.starts_with("x-roman")
}

/// A background vocal: a second voice singing over the line it sits inside.
fn is_background_role(role: &str) -> bool {
    role.starts_with("x-bg")
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

/// One run of text being accumulated: a `<p>`, or the background vocal inside
/// one. Both are built the same way, so text, entities, `<br/>` and word tags
/// have exactly one code path between them.
#[derive(Default)]
struct Buf {
    begin: Option<f64>,
    end: Option<f64>,
    /// A2 body: word tags interleaved with the literal text between them.
    body: String,
    words: usize,
}

impl Buf {
    /// Widen the times from a span inside this run. An `x-bg` wrapper usually
    /// carries its own `begin`/`end`, but not always; where it does not, its
    /// spans are the only record of when the phrase runs.
    fn widen(&mut self, begin: Option<f64>, end: Option<f64>) {
        if let Some(b) = begin
            && self.begin.is_none_or(|cur| b < cur)
        {
            self.begin = Some(b);
        }
        if let Some(e) = end
            && self.end.is_none_or(|cur| e > cur)
        {
            self.end = Some(e);
        }
    }
}

/// One `<p>`: the line itself, plus any voices singing over it.
struct OutLine {
    line: Buf,
    backgrounds: Vec<Buf>,
}

/// What a `<span>` on the stack is, so that skipping and capturing stay
/// independent. They have to be: the real database nests a translation *inside*
/// a background vocal, and a single depth counter cannot both keep the
/// background and drop the translation within it.
enum SpanKind {
    /// A translation or romanisation, and everything under it.
    Skipped,
    /// The `x-bg` wrapper. Its contents accumulate into a buffer of their own.
    Background,
    /// An ordinary timed span; the payload is its `end`, used to close the word.
    Word(Option<f64>),
}

/// Convert a TTML document into an enhanced-LRC string.
pub fn to_enhanced_lrc(xml: &str) -> Result<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut out: Vec<OutLine> = Vec::new();
    let mut line: Option<OutLine> = None;
    // The background vocal currently being accumulated, if we are inside one.
    let mut bg: Option<Buf> = None;
    // Depth of nested spans we are ignoring (a translation or a romanisation).
    let mut skip_depth = 0usize;
    let mut span_stack: Vec<SpanKind> = Vec::new();
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
                    && let Some(b) = open_buf(&mut bg, &mut line)
                {
                    b.body.push(' ');
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
                        line = Some(OutLine {
                            line: Buf {
                                begin,
                                end: attr(&e, "end").as_deref().and_then(parse_time),
                                ..Default::default()
                            },
                            backgrounds: Vec::new(),
                        });
                    }
                    "span" => {
                        let role = attr(&e, "role").unwrap_or_default();
                        // The skip test comes first, so a translation nested
                        // inside a background vocal is still dropped.
                        if skip_depth > 0 || is_skippable_role(&role) {
                            skip_depth += 1;
                            span_stack.push(SpanKind::Skipped);
                            continue;
                        }
                        let begin = attr(&e, "begin").and_then(|v| parse_time(&v));
                        let end = attr(&e, "end").and_then(|v| parse_time(&v));
                        // An `x-bg` inside an `x-bg` is not a thing; treat the
                        // inner one as an ordinary span rather than losing the
                        // capture already in progress.
                        if is_background_role(&role) && bg.is_none() {
                            bg = Some(Buf {
                                begin,
                                end,
                                ..Default::default()
                            });
                            span_stack.push(SpanKind::Background);
                            continue;
                        }
                        if let Some(b) = open_buf(&mut bg, &mut line) {
                            if let Some(t) = begin {
                                b.body.push_str(&format!("<{}>", format_time(t)));
                                b.words += 1;
                            }
                            b.widen(begin, end);
                        }
                        span_stack.push(SpanKind::Word(end));
                    }
                    _ => {}
                }
            }

            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                "span" => match span_stack.pop() {
                    Some(SpanKind::Skipped) => skip_depth -= 1,
                    Some(SpanKind::Background) => {
                        if let (Some(done), Some(l)) = (bg.take(), line.as_mut()) {
                            l.backgrounds.push(done);
                        }
                    }
                    // Close the word so a gap before the next one is honoured.
                    Some(SpanKind::Word(end)) => {
                        if let (Some(b), Some(end)) = (open_buf(&mut bg, &mut line), end) {
                            b.body.push_str(&format!("<{}>", format_time(end)));
                        }
                    }
                    None => {}
                },
                "p" => {
                    // An unclosed background would otherwise leak into the next
                    // line and take its text with it.
                    if let (Some(done), Some(l)) = (bg.take(), line.as_mut()) {
                        l.backgrounds.push(done);
                    }
                    if let Some(l) = line.take() {
                        out.push(l);
                    }
                }
                _ => {}
            },

            Ok(Event::Text(t)) => {
                if skip_depth > 0 {
                    continue;
                }
                if let Some(b) = open_buf(&mut bg, &mut line) {
                    push_text(&mut b.body, t.xml10_content().as_ref());
                }
            }

            // Entities arrive as their own events rather than inside the text,
            // so `don&apos;t` would otherwise come through as `dont`.
            Ok(Event::GeneralRef(r)) => {
                if skip_depth > 0 {
                    continue;
                }
                if let Some(b) = open_buf(&mut bg, &mut line)
                    && let Some(resolved) = resolve_entity(r.xml10_content().as_ref())
                {
                    b.body.push_str(&resolved);
                }
            }

            _ => {}
        }
    }

    let out = render_lines(&out);

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

/// Whichever run of text is currently open: the background vocal if we are
/// inside one, otherwise the line itself.
fn open_buf<'a>(bg: &'a mut Option<Buf>, line: &'a mut Option<OutLine>) -> Option<&'a mut Buf> {
    match bg {
        Some(b) => Some(b),
        None => line.as_mut().map(|l| &mut l.line),
    }
}

/// Write the accumulated lines out as enhanced LRC.
///
/// Each `<p>` becomes its line followed by its background vocals. A background
/// names its parent by that line's start rather than relying on adjacency,
/// because `lrc::finalise` sorts by time and a background that comes in after
/// the next line has started would otherwise attach to the wrong one.
///
/// A background whose time cannot be read is dropped rather than failing the
/// document. It is texture: losing the whole set of lyrics over it would be the
/// wrong trade, which is the opposite of the call made for a `<p>` — a line
/// nobody can place in time still fails loudly.
fn render_lines(lines: &[OutLine]) -> String {
    let mut out = String::new();
    for l in lines {
        let Some(begin) = l.line.begin else { continue };
        out.push_str(&format!("[{}]", format_time(begin)));
        if let Some(end) = l.line.end {
            out.push_str(&format!("[end:{}]", format_time(end)));
        }
        out.push_str(l.line.body.trim());
        out.push('\n');

        for b in &l.backgrounds {
            let Some(bg_begin) = b.begin else { continue };
            let body = b.body.trim();
            if body.is_empty() {
                continue;
            }
            out.push_str(&format!(
                "[{}][bg:{}]",
                format_time(bg_begin),
                format_time(begin)
            ));
            if let Some(end) = b.end {
                out.push_str(&format!("[end:{}]", format_time(end)));
            }
            out.push_str(body);
            out.push('\n');
        }
    }
    out
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
