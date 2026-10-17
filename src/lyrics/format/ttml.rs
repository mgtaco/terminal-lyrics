//! Apple-style timed TTML: `ttm:agent` voices, `x-bg` backing vocals, and
//! word timing from `<span begin>`.
//!
//! Every element hands a scope down to its children — its times, whether it is
//! a backing vocal, whose voice it is — and text lands in the line for its
//! voice within the current `<p>`. A lead and its backing vocal are two lines
//! from one paragraph, tied together by [`Line::group`].

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

use super::{MAX_LINES, MAX_WORDS, decode};
use crate::lyrics::error::{Error, Result};
use crate::lyrics::model::{
    Line, Lyrics, Seconds, SourceFormat, Voice, Word, bounded_voice_id, join_words, sort_lines,
    trim_words, voice_metadata,
};

const MAX_DEPTH: usize = 32;

fn invalid() -> Error {
    Error::invalid("invalid timed lyrics document")
}

/// What an element hands down to its children.
#[derive(Debug, Clone, Default)]
struct Scope {
    start: Seconds,
    end: Seconds,
    timed: bool,
    word_timed: bool,
    skipped: bool,
    background: bool,
    agent_name: bool,
    voice: String,
}

/// One voice within a `<p>`.
struct Part {
    start: Seconds,
    end: Seconds,
    voice: String,
    background: bool,
    word_timed: bool,
    words: Vec<PartWord>,
}

struct PartWord {
    start: Seconds,
    end: Seconds,
    text: String,
}

#[derive(Default)]
struct Parser {
    scopes: Vec<Scope>,
    root_seen: bool,
    in_body: bool,
    in_paragraph: bool,
    parts: Vec<Part>,
    paragraphs: usize,
    words: usize,

    agent: String,
    agent_name: String,

    plain: Vec<String>,
    synced: Vec<Line>,
    voices: std::collections::BTreeMap<String, Voice>,
}

pub fn parse(data: &[u8]) -> Result<Lyrics> {
    let text = decode(data)?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    reader.config_mut().check_end_names = true;

    let mut parser = Parser::default();
    // Character data arrives in pieces — entities are events of their own — but
    // the parser has to see it the way one run of text reads.
    let mut pending = String::new();
    loop {
        let event = reader.read_event().map_err(Error::invalid)?;
        let is_text = matches!(
            event,
            Event::Text(_) | Event::GeneralRef(_) | Event::CData(_)
        );
        if !is_text && !pending.is_empty() {
            parser.text(&std::mem::take(&mut pending))?;
        }
        match event {
            Event::Eof => break,
            Event::Start(e) => parser.open(&e)?,
            Event::Empty(e) => {
                parser.open(&e)?;
                parser.close(local_name(e.name().as_ref()))?;
            }
            Event::End(e) => parser.close(local_name(e.name().as_ref()))?,
            Event::Text(t) => pending.push_str(&t.xml10_content()),
            Event::CData(t) => pending.push_str(&t.into_inner()),
            Event::GeneralRef(r) => {
                let name = r.xml10_content();
                pending.push_str(&resolve_entity(&name).ok_or_else(invalid)?);
            }
            Event::DocType(_) => return Err(invalid()),
            _ => {}
        }
    }
    if !parser.root_seen || !parser.scopes.is_empty() {
        return Err(invalid());
    }

    sort_lines(&mut parser.synced);
    Ok(Lyrics {
        synced: parser.synced,
        plain: parser.plain.join("\n"),
        voices: parser.voices,
        source: SourceFormat::Ttml,
        ..Default::default()
    })
}

impl Parser {
    fn open(&mut self, element: &BytesStart<'_>) -> Result<()> {
        if self.scopes.len() >= MAX_DEPTH {
            return Err(Error::TooLarge);
        }
        let name = local_name(element.name().as_ref()).to_string();
        let mut scope = match self.scopes.last() {
            Some(parent) => parent.clone(),
            None if self.root_seen || name != "tt" => return Err(invalid()),
            None => Scope::default(),
        };
        self.root_seen = true;

        let mut id = String::new();
        let mut kind = String::new();
        let mut duration: Seconds = 0.0;
        for attr in element.attributes() {
            let attr = attr.map_err(Error::invalid)?;
            let key = local_name(attr.key.as_ref()).to_string();
            let value = attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(Error::invalid)?
                .to_string();
            match key.as_str() {
                "begin" => {
                    scope.start = seconds(&value)?;
                    scope.timed = true;
                    scope.word_timed = name == "span";
                }
                "end" => scope.end = seconds(&value)?,
                "dur" => duration = seconds(&value)?,
                "role" => match value.as_str() {
                    "x-translation" | "x-transliteration" => scope.skipped = true,
                    "x-bg" => scope.background = true,
                    _ => {}
                },
                "agent" => scope.voice = bounded_voice_id(&value),
                "id" => id = bounded_voice_id(&value),
                "type" => kind = value,
                _ => {}
            }
        }
        if duration > 0.0 {
            scope.end = scope.start + duration;
        }
        if !scope.end.is_finite() {
            return Err(invalid());
        }

        if !self.in_body {
            if name == "agent" && !id.is_empty() {
                if self.voices.len() >= MAX_LINES {
                    return Err(Error::TooLarge);
                }
                self.voices.insert(id.clone(), voice_metadata("", &kind));
                self.agent = id;
            } else if name == "name"
                && !self.agent.is_empty()
                && (kind == "full"
                    || self
                        .voices
                        .get(&self.agent)
                        .is_none_or(|v| v.name.is_empty()))
            {
                scope.agent_name = true;
                self.agent_name.clear();
            }
        }

        let skipped = scope.skipped;
        self.scopes.push(scope);
        if name == "body" {
            self.in_body = true;
        }
        if !self.in_body || skipped {
            return Ok(());
        }
        match name.as_str() {
            "p" => {
                if self.in_paragraph {
                    return Err(invalid());
                }
                if self.paragraphs >= MAX_LINES {
                    return Err(Error::TooLarge);
                }
                self.paragraphs += 1;
                self.in_paragraph = true;
            }
            "br" => return self.text("\n"),
            _ => {}
        }
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<()> {
        let Some(scope) = self.scopes.last().cloned() else {
            return Ok(());
        };
        if text.is_empty() {
            return Ok(());
        }
        if scope.agent_name {
            self.agent_name.push_str(text);
        }
        if !self.in_paragraph || scope.skipped {
            return Ok(());
        }
        let text = if text == "\n" {
            text.to_string()
        } else {
            text.chars()
                .map(|c| if c.is_whitespace() { ' ' } else { c })
                .collect()
        };
        let blank = text.trim().is_empty();

        let index = match self
            .parts
            .iter()
            .position(|p| p.voice == scope.voice && p.background == scope.background)
        {
            Some(index) => index,
            None => {
                if blank {
                    return Ok(());
                }
                if self.parts.len() >= MAX_LINES {
                    return Err(Error::TooLarge);
                }
                self.parts.push(Part {
                    start: -1.0,
                    end: 0.0,
                    voice: scope.voice.clone(),
                    background: scope.background,
                    word_timed: false,
                    words: Vec::new(),
                });
                self.parts.len() - 1
            }
        };
        let line = &mut self.parts[index];
        line.word_timed = line.word_timed || scope.word_timed;

        let at = if scope.timed { scope.start } else { -1.0 };
        if scope.timed && !blank {
            if line.start < 0.0 || at < line.start {
                line.start = at;
            }
            if scope.end > at {
                line.end = line.end.max(scope.end);
            }
        }

        if let Some(last) = line.words.last_mut()
            && (!scope.word_timed || last.start == at)
        {
            last.text.push_str(&text);
            return Ok(());
        }
        self.words += 1;
        if self.words > MAX_WORDS {
            return Err(Error::TooLarge);
        }
        line.words.push(PartWord {
            start: at,
            end: scope.end,
            text,
        });
        Ok(())
    }

    fn close(&mut self, name: &str) -> Result<()> {
        let Some(scope) = self.scopes.pop() else {
            return Err(invalid());
        };
        match name {
            "name" if scope.agent_name && !self.agent.is_empty() => {
                let kind = self
                    .voices
                    .get(&self.agent)
                    .map(|v| v.kind.clone())
                    .unwrap_or_default();
                self.voices.insert(
                    self.agent.clone(),
                    voice_metadata(self.agent_name.trim(), &kind),
                );
            }
            "agent" => self.agent.clear(),
            "body" => self.in_body = false,
            "p" if self.in_paragraph && !scope.skipped => return self.close_paragraph(),
            _ => {}
        }
        Ok(())
    }

    fn close_paragraph(&mut self) -> Result<()> {
        let parts = std::mem::take(&mut self.parts);
        let grouped = parts.len() > 1;
        for part in parts {
            let mut line = part.finish();
            if line.text.is_empty() {
                continue;
            }
            self.plain.push(line.text.clone());
            if self.plain.len() > MAX_LINES {
                return Err(Error::TooLarge);
            }
            if grouped {
                line.group = self.paragraphs as i64;
            }
            if line.start >= 0.0 {
                self.synced.push(line);
            }
        }
        self.in_paragraph = false;
        Ok(())
    }
}

impl Part {
    fn finish(self) -> Line {
        let words: Vec<Word> = self
            .words
            .into_iter()
            .map(|source| {
                let start = source.start.max(self.start);
                Word {
                    start,
                    end: if source.end > start { source.end } else { 0.0 },
                    text: source.text,
                }
            })
            .collect();
        let words = trim_words(words);
        Line {
            start: self.start,
            end: self.end,
            text: join_words(&words),
            words: if self.word_timed { words } else { Vec::new() },
            voice: self.voice,
            background: self.background,
            group: 0,
        }
    }
}

/// A TTML time: a clock (`1:04.579`) or an offset with an optional metric
/// (`4.658s`, `250ms`).
fn seconds(value: &str) -> Result<Seconds> {
    let mut value = value;
    let mut scale = 1.0;
    for (suffix, unit) in [("ms", 0.001), ("s", 1.0), ("m", 60.0), ("h", 3600.0)] {
        if let Some(rest) = value.strip_suffix(suffix) {
            value = rest;
            scale = unit;
            break;
        }
    }
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() > 3 {
        return Err(invalid());
    }
    let mut total = 0.0;
    for part in parts {
        match part.parse::<f64>() {
            Ok(n) if n.is_finite() && n >= 0.0 => total = total * 60.0 + n,
            _ => return Err(invalid()),
        }
    }
    total *= scale;
    if total.is_infinite() {
        return Err(invalid());
    }
    Ok(total)
}

/// The five predefined entities and numeric references; nothing else, as in
/// any XML parser that has not read a DTD.
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
    let code = match digits.strip_prefix('x') {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => digits.parse().ok()?,
    };
    char::from_u32(code).map(String::from)
}

/// `ttm:agent` is `agent`: everything here matches on the local part.
fn local_name(raw: &str) -> &str {
    match raw.rsplit_once(':') {
        Some((_, local)) => local,
        None => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata">
<head><metadata><ttm:agent type="person" xml:id="v1"><ttm:name type="full">Lead</ttm:name></ttm:agent></metadata></head>
<body dur="3:00.000"><div>
<p begin="1.0" end="3.0" ttm:agent="v1"><span begin="1.0" end="1.5">Hel</span><span begin="1.5" end="2.0">lo</span> <span begin="2.0" end="3.0">you</span><span ttm:role="x-bg"><span begin="2.5" end="3.5">(ooh)</span></span></p>
<p begin="4.0" end="5.0"><span begin="4.0" end="5.0">Don&apos;t</span></p>
</div></body></tt>"#;

    #[test]
    fn words_voices_and_backing_vocals() {
        let parsed = parse(DOC.as_bytes()).unwrap();
        assert_eq!(parsed.voices["v1"].name, "Lead");
        assert_eq!(parsed.voices["v1"].kind, "person");

        let lead = &parsed.synced[0];
        assert_eq!(lead.text, "Hello you");
        let words: Vec<_> = lead.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(words, ["Hel", "lo ", "you"]);
        assert_eq!(lead.voice, "v1");
        assert_eq!(lead.group, 1);

        let bg = &parsed.synced[1];
        assert!(bg.background);
        assert_eq!(bg.text, "(ooh)");
        assert_eq!(bg.start, 2.5);
        assert_eq!(bg.group, 1);

        assert_eq!(parsed.synced[2].text, "Don't");
        assert_eq!(parsed.synced[2].group, 0);
    }

    #[test]
    fn translations_are_skipped() {
        let doc = r#"<tt><body><p begin="1s"><span begin="1s">hola</span><span ttm:role="x-translation">hello</span></p></body></tt>"#;
        let parsed = parse(doc.as_bytes()).unwrap();
        assert_eq!(parsed.synced[0].text, "hola");
    }

    #[test]
    fn line_timed_paragraphs_have_no_words() {
        let doc = r#"<tt><body><p begin="00:01.000" end="00:02.000">just a line</p></body></tt>"#;
        let parsed = parse(doc.as_bytes()).unwrap();
        assert_eq!(parsed.synced[0].start, 1.0);
        assert!(parsed.synced[0].words.is_empty());
    }

    #[test]
    fn not_ttml_is_an_error() {
        assert!(parse(b"<html><body/></html>").is_err());
        assert!(parse(b"<tt><body><p begin=\"x\">a</p></body></tt>").is_err());
        assert!(parse(b"<tt><body>").is_err());
    }
}
