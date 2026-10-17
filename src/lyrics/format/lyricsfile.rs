//! Lyricsfile 1.0: YAML with per-line and per-word millisecond stamps.
//!
//! LRCLIB serves these for some records, and a `.lyricsfile.yaml` next to the
//! track is read too. The document is untrusted YAML, so only the plain subset
//! is accepted: no anchors or aliases, no tags beyond the core string, int,
//! bool and null, unique string keys, bounded depth and width.

use yaml_rust2::parser::{Event, EventReceiver, Parser};
use yaml_rust2::scanner::TScalarStyle;

use super::{MAX_LINES, MAX_WORDS, decode};
use crate::lyrics::error::{Error, Result};
use crate::lyrics::model::{
    Line, Lyrics, SourceFormat, Word, join_words, milliseconds, sort_lines,
};

fn invalid() -> Error {
    Error::invalid("invalid Lyricsfile document")
}

const MAX_DEPTH: usize = 16;
/// Keys plus values, so 64 pairs.
const MAX_MAPPING_ITEMS: usize = 128;

/// A YAML value with its core-schema type already resolved.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Seq(Vec<Node>),
    Map(Vec<(String, Node)>),
}

impl Node {
    fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// A string field: absent and null are empty, and an int or bool reads as
    /// the way it was written.
    fn string(node: Option<&Node>) -> Result<String> {
        match node {
            None | Some(Node::Null) => Ok(String::new()),
            Some(Node::Str(s)) => Ok(s.clone()),
            Some(Node::Int(i)) => Ok(i.to_string()),
            Some(Node::Bool(b)) => Ok(b.to_string()),
            _ => Err(invalid()),
        }
    }

    fn int(node: Option<&Node>) -> Result<Option<i64>> {
        match node {
            None | Some(Node::Null) => Ok(None),
            Some(Node::Int(i)) => Ok(Some(*i)),
            _ => Err(invalid()),
        }
    }

    fn boolean(node: Option<&Node>) -> Result<bool> {
        match node {
            None | Some(Node::Null) => Ok(false),
            Some(Node::Bool(b)) => Ok(*b),
            _ => Err(invalid()),
        }
    }

    fn seq(node: Option<&Node>) -> Result<&[Node]> {
        match node {
            None | Some(Node::Null) => Ok(&[]),
            Some(Node::Seq(items)) => Ok(items),
            _ => Err(invalid()),
        }
    }
}

enum Frame {
    Seq(Vec<Node>),
    Map(Vec<(String, Node)>, Option<String>),
}

/// Builds [`Node`]s from parser events, refusing anything outside the subset.
#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    documents: Vec<Node>,
    error: Option<Error>,
}

impl Builder {
    fn fail(&mut self, error: Error) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    fn push(&mut self, node: Node) {
        match self.stack.last_mut() {
            None => self.documents.push(node),
            Some(Frame::Seq(items)) => items.push(node),
            Some(Frame::Map(items, key)) => match key.take() {
                Some(k) => {
                    items.push((k, node));
                    if items.len() * 2 > MAX_MAPPING_ITEMS {
                        self.fail(Error::TooLarge);
                    }
                }
                None => match node {
                    Node::Str(k) if !items.iter().any(|(existing, _)| *existing == k) => {
                        *key = Some(k)
                    }
                    _ => self.fail(invalid()),
                },
            },
        }
    }

    fn open(&mut self, anchor: usize, tag: Option<&yaml_rust2::parser::Tag>, seq: bool) {
        if anchor != 0 {
            return self.fail(invalid());
        }
        let want = if seq { "seq" } else { "map" };
        if let Some(tag) = tag
            && !core_tag(tag, want)
        {
            return self.fail(invalid());
        }
        // The document is depth 0, so the root mapping is 1.
        if self.stack.len() + 1 > MAX_DEPTH {
            return self.fail(Error::TooLarge);
        }
        self.stack.push(if seq {
            Frame::Seq(Vec::new())
        } else {
            Frame::Map(Vec::new(), None)
        });
    }

    fn close(&mut self) {
        let node = match self.stack.pop() {
            Some(Frame::Seq(items)) => Node::Seq(items),
            Some(Frame::Map(items, _)) => Node::Map(items),
            None => return self.fail(invalid()),
        };
        self.push(node);
    }
}

fn core_tag(tag: &yaml_rust2::parser::Tag, suffix: &str) -> bool {
    tag.handle == "tag:yaml.org,2002:" && tag.suffix == suffix
}

impl EventReceiver for Builder {
    fn on_event(&mut self, event: Event) {
        if self.error.is_some() {
            return;
        }
        match event {
            Event::Alias(_) => self.fail(invalid()),
            Event::SequenceStart(anchor, tag) => self.open(anchor, tag.as_ref(), true),
            Event::MappingStart(anchor, tag) => self.open(anchor, tag.as_ref(), false),
            Event::SequenceEnd | Event::MappingEnd => self.close(),
            Event::Scalar(value, style, anchor, tag) => {
                if anchor != 0 {
                    return self.fail(invalid());
                }
                if self.stack.len() + 1 > MAX_DEPTH {
                    return self.fail(Error::TooLarge);
                }
                match scalar(value, style, tag.as_ref()) {
                    Some(node) => self.push(node),
                    None => self.fail(invalid()),
                }
            }
            _ => {}
        }
    }
}

/// Resolve a scalar the way the YAML core schema does. Floats and every other
/// tag are outside the subset, so they come back `None`.
fn scalar(
    value: String,
    style: TScalarStyle,
    tag: Option<&yaml_rust2::parser::Tag>,
) -> Option<Node> {
    if let Some(tag) = tag {
        if tag.handle != "tag:yaml.org,2002:" {
            return None;
        }
        return match tag.suffix.as_str() {
            "str" => Some(Node::Str(value)),
            "int" => resolve_int(&value).map(Node::Int),
            "bool" => resolve_bool(&value).map(Node::Bool),
            "null" => Some(Node::Null),
            _ => None,
        };
    }
    if style != TScalarStyle::Plain {
        return Some(Node::Str(value));
    }
    if matches!(value.as_str(), "" | "~" | "null" | "Null" | "NULL") {
        return Some(Node::Null);
    }
    if let Some(b) = resolve_bool(&value) {
        return Some(Node::Bool(b));
    }
    if let Some(i) = resolve_int(&value) {
        return Some(Node::Int(i));
    }
    if looks_like_float(&value) {
        return None;
    }
    Some(Node::Str(value))
}

fn resolve_bool(value: &str) -> Option<bool> {
    match value {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

fn resolve_int(value: &str) -> Option<i64> {
    let (negative, digits) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let (radix, digits) = if let Some(hex) = digits.strip_prefix("0x") {
        (16, hex)
    } else if let Some(oct) = digits.strip_prefix("0o") {
        (8, oct)
    } else if let Some(bin) = digits.strip_prefix("0b") {
        (2, bin)
    } else {
        (10, digits)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let magnitude = i64::from_str_radix(digits, radix).ok()?;
    Some(if negative { -magnitude } else { magnitude })
}

fn looks_like_float(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let unsigned = lower.trim_start_matches(['+', '-']);
    matches!(unsigned, ".inf" | ".nan") || value.parse::<f64>().is_ok()
}

/// A line or a word: its text and its stamps, end optional.
struct Span {
    text: String,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
}

impl Span {
    fn read(node: &Node) -> Result<Self> {
        if !matches!(node, Node::Map(_)) {
            return Err(invalid());
        }
        Ok(Span {
            text: Node::string(node.get("text"))?,
            start_ms: Node::int(node.get("start_ms"))?,
            end_ms: Node::int(node.get("end_ms"))?,
        })
    }

    fn timing(&self) -> Option<(f64, f64)> {
        let start = self.start_ms.filter(|s| *s >= 0)?;
        match self.end_ms {
            None => Some((milliseconds(start), 0.0)),
            Some(end) if end >= start => Some((milliseconds(start), milliseconds(end))),
            Some(_) => None,
        }
    }
}

pub fn parse(data: &[u8]) -> Result<Lyrics> {
    let text = decode(data)?;
    let mut builder = Builder::default();
    Parser::new_from_str(text)
        .load(&mut builder, true)
        .map_err(Error::invalid)?;
    if let Some(error) = builder.error {
        return Err(error);
    }
    let [document] = builder.documents.as_slice() else {
        return Err(invalid());
    };
    if !matches!(document, Node::Map(_)) {
        return Err(invalid());
    }

    if Node::string(document.get("version"))? != "1.0" {
        return Err(invalid());
    }
    let metadata = match document.get("metadata") {
        None | Some(Node::Null) => &Node::Map(Vec::new()),
        Some(node @ Node::Map(_)) => node,
        Some(_) => return Err(invalid()),
    };
    let mut result = Lyrics {
        title: Node::string(metadata.get("title"))?,
        artist: Node::string(metadata.get("artist"))?,
        album: Node::string(metadata.get("album"))?,
        duration_ms: Node::int(metadata.get("duration_ms"))?.unwrap_or(0),
        source: SourceFormat::Lyricsfile,
        ..Default::default()
    };
    let lines = Node::seq(document.get("lines"))?;
    let plain = Node::string(document.get("plain"))?;
    if Node::boolean(metadata.get("instrumental"))? {
        result.instrumental = true;
        return Ok(result);
    }
    if lines.len() > MAX_LINES || plain.matches('\n').count() + 1 > MAX_LINES {
        return Err(Error::TooLarge);
    }
    result.plain = plain;

    let mut word_count = 0;
    for source in lines {
        let span = Span::read(source)?;
        let (start, end) = span.timing().ok_or_else(invalid)?;
        let spans = Node::seq(source.get("words"))?;
        word_count += spans.len();
        if word_count > MAX_WORDS {
            return Err(Error::TooLarge);
        }
        let mut line = Line {
            start,
            end,
            text: span.text,
            ..Default::default()
        };
        let words = words(spans)?;
        let words_text = join_words(&words);
        if line.text.is_empty() {
            line.text = words_text.clone();
        }
        if words_text == line.text && !words_text.trim().is_empty() {
            for word in &words {
                line.start = line.start.min(word.start);
            }
            line.words = words;
        }
        result.synced.push(line);
    }
    sort_lines(&mut result.synced);
    if result.plain.is_empty() {
        result.plain = result
            .synced
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
    }
    Ok(result)
}

/// The words, or none when any of them is untimed.
fn words(spans: &[Node]) -> Result<Vec<Word>> {
    let mut words = Vec::with_capacity(spans.len());
    for node in spans {
        let span = Span::read(node)?;
        let Some((start, end)) = span.timing() else {
            return Ok(Vec::new());
        };
        words.push(Word {
            start,
            end,
            text: span.text,
        });
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"version: "1.0"
metadata:
  title: Get Lucky
  artist: Daft Punk
  duration_ms: 369000
lines:
  - text: Like the legend
    start_ms: 31400
    end_ms: 35200
    words:
      - { text: "Like ", start_ms: 31400, end_ms: 31900 }
      - { text: "the ", start_ms: 31900, end_ms: 32300 }
      - { text: "legend", start_ms: 32300, end_ms: 35200 }
  - text: Second line
    start_ms: 36000
"#;

    #[test]
    fn reads_lines_and_words() {
        let parsed = parse(DOC.as_bytes()).unwrap();
        assert_eq!(parsed.title, "Get Lucky");
        assert_eq!(parsed.duration_ms, 369000);
        assert_eq!(parsed.synced.len(), 2);
        assert_eq!(parsed.synced[0].words.len(), 3);
        assert_eq!(parsed.synced[0].start, 31.4);
        assert!(parsed.synced[1].words.is_empty());
        assert_eq!(parsed.plain, "Like the legend\nSecond line");
    }

    #[test]
    fn refuses_what_is_outside_the_subset() {
        let anchored = "version: \"1.0\"\nmetadata: &m {title: a}\nother: *m\n";
        assert!(parse(anchored.as_bytes()).is_err());
        let duplicate = "version: \"1.0\"\nversion: \"1.0\"\n";
        assert!(parse(duplicate.as_bytes()).is_err());
        let float = "version: 1.0\n";
        assert!(parse(float.as_bytes()).is_err());
        let two = "version: \"1.0\"\n---\nversion: \"1.0\"\n";
        assert!(parse(two.as_bytes()).is_err());
    }

    #[test]
    fn words_that_do_not_spell_the_line_are_dropped() {
        let doc = "version: \"1.0\"\nlines:\n  - text: abc\n    start_ms: 10\n    words:\n      - {text: x, start_ms: 10}\n";
        let parsed = parse(doc.as_bytes()).unwrap();
        assert!(parsed.synced[0].words.is_empty());
    }
}
