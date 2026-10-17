//! LyricsPlus (the binimum.org instance): JSON with per-syllable timing and
//! backing vocals flagged word by word.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;

use super::Http;
use crate::lyrics::error::{Error, Result};
use crate::lyrics::format::{MAX_LINES, MAX_WORDS};
use crate::lyrics::model::{
    Line, Lyrics, Request, Voice, Word, bounded_voice_id, join_words, milliseconds, sort_lines,
    trim_words, voice_metadata,
};

const URL: &str = "https://lyricsplus.binimum.org/v2/lyrics/get";

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Payload {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    lyrics: Option<Vec<SourceLine>>,
    #[serde(default)]
    metadata: Option<Metadata>,
}

#[derive(Debug, Default, Deserialize)]
struct Metadata {
    #[serde(default)]
    agents: Option<BTreeMap<String, Agent>>,
}

#[derive(Debug, Default, Deserialize)]
struct Agent {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    alias: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SourceLine {
    #[serde(default)]
    time: Option<i64>,
    #[serde(default)]
    duration: Option<i64>,
    #[serde(default)]
    text: Option<String>,
    #[serde(rename = "syllabus", default)]
    words: Option<Vec<SourceWord>>,
    #[serde(default)]
    element: Option<Element>,
}

#[derive(Debug, Default, Deserialize)]
struct Element {
    #[serde(default)]
    singer: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SourceWord {
    #[serde(default)]
    time: Option<i64>,
    #[serde(default)]
    duration: Option<i64>,
    #[serde(default)]
    text: Option<String>,
    #[serde(rename = "isBackground", default)]
    background: Option<bool>,
}

pub async fn fetch(http: &Http, req: &Request) -> Result<Lyrics> {
    let query = req.query("title", "artist", "album", "duration");
    let payload: Payload = http.get_json(URL, &query, Duration::from_secs(4)).await?;
    decode(payload)
}

pub(crate) fn decode(payload: Payload) -> Result<Lyrics> {
    let source = payload.lyrics.unwrap_or_default();
    let agents = payload.metadata.and_then(|m| m.agents).unwrap_or_default();
    if source.len() > MAX_LINES || agents.len() > MAX_LINES {
        return Err(Error::TooLarge);
    }
    let kind = payload.kind.unwrap_or_default();
    let timed = kind == "Word" || kind == "Line";
    let mut result = Lyrics {
        voices: voices(agents),
        ..Default::default()
    };
    let mut plain = Vec::with_capacity(source.len());
    let mut word_count = 0;
    for (index, line_source) in source.iter().enumerate() {
        let text = line_source.text.clone().unwrap_or_default();
        plain.push(text.clone());
        let syllables = line_source.words.as_deref().unwrap_or_default();
        word_count += syllables.len();
        if word_count > MAX_WORDS {
            return Err(Error::TooLarge);
        }
        let Some(time) = line_source.time.filter(|t| timed && *t >= 0) else {
            continue;
        };
        let singer = line_source
            .element
            .as_ref()
            .and_then(|e| e.singer.as_deref())
            .unwrap_or_default();
        let mut line = Line {
            start: milliseconds(time),
            text: text.clone(),
            voice: bounded_voice_id(singer),
            ..Default::default()
        };
        let duration = line_source.duration.unwrap_or(0);
        if duration > 0 {
            line.end = line.start + milliseconds(duration);
        }
        if kind == "Word" {
            line.words = words(time, &text, syllables);
        }
        if line.words.is_empty() {
            result.synced.push(line);
            continue;
        }
        let parts = split_background(&line, syllables);
        let grouped = parts.len() > 1;
        for mut part in parts {
            if grouped {
                part.group = index as i64 + 1;
            }
            result.synced.push(part);
        }
    }
    if result.synced.len() > MAX_LINES {
        return Err(Error::TooLarge);
    }
    if !result.synced.is_empty() && !result.timed() {
        return Err(Error::UntimedSync);
    }
    sort_lines(&mut result.synced);
    result.plain = plain.join("\n");
    Ok(result)
}

fn voices(agents: BTreeMap<String, Agent>) -> BTreeMap<String, Voice> {
    let mut voices = BTreeMap::new();
    for (name, agent) in agents {
        let name = match agent.alias.as_deref() {
            Some(alias) if !alias.is_empty() => alias.to_string(),
            _ => name,
        };
        let id = bounded_voice_id(&name);
        if !id.is_empty() {
            voices.insert(
                id,
                voice_metadata(
                    agent.name.as_deref().unwrap_or_default(),
                    agent.kind.as_deref().unwrap_or_default(),
                ),
            );
        }
    }
    voices
}

/// Empty unless every syllable is timed inside the line and together they
/// spell out its text.
fn words(line_time: i64, text: &str, syllables: &[SourceWord]) -> Vec<Word> {
    let mut words = Vec::with_capacity(syllables.len());
    for source in syllables {
        let Some(time) = source.time.filter(|t| *t >= line_time) else {
            return Vec::new();
        };
        let mut word = Word {
            start: milliseconds(time),
            end: 0.0,
            text: source.text.clone().unwrap_or_default(),
        };
        let duration = source.duration.unwrap_or(0);
        if duration > 0 {
            word.end = word.start + milliseconds(duration);
        }
        words.push(word);
    }
    if words.is_empty() || join_words(&words) != text {
        return Vec::new();
    }
    words
}

/// The lead and its backing vocal as two lines, when the syllables say there
/// is one.
fn split_background(line: &Line, source: &[SourceWord]) -> Vec<Line> {
    let is_bg = |w: &SourceWord| w.background.unwrap_or(false);
    if !source.iter().any(is_bg) {
        return vec![line.clone()];
    }
    let mut parts = Vec::new();
    for background in [false, true] {
        let words: Vec<Word> = line
            .words
            .iter()
            .zip(source)
            .filter(|(_, s)| is_bg(s) == background)
            .map(|(w, _)| w.clone())
            .collect();
        let words = trim_words(words);
        if words.is_empty() {
            continue;
        }
        let mut part = Line {
            voice: line.voice.clone(),
            background,
            start: words[0].start,
            ..Default::default()
        };
        for word in &words {
            part.start = part.start.min(word.start);
            part.end = part.end.max(word.end);
        }
        part.text = join_words(&words);
        part.words = words;
        parts.push(part);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(json: &str) -> Payload {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn syllables_and_backing_vocals() {
        let doc = r#"{"type":"Word","lyrics":[{"time":1000,"duration":2000,"text":"Hello (ooh)",
            "syllabus":[{"time":1000,"duration":500,"text":"Hel"},{"time":1500,"duration":500,"text":"lo "},
            {"time":2000,"duration":500,"text":"(ooh)","isBackground":true}],"element":{"singer":"v1"}}],
            "metadata":{"agents":{"v1":{"name":"Lead","type":"person"}}}}"#;
        let parsed = decode(payload(doc)).unwrap();
        assert_eq!(parsed.synced.len(), 2);
        assert_eq!(parsed.synced[0].text, "Hello");
        assert_eq!(parsed.synced[0].group, 1);
        assert!(parsed.synced[1].background);
        assert_eq!(parsed.synced[1].text, "(ooh)");
        assert_eq!(parsed.voices["v1"].name, "Lead");
    }

    #[test]
    fn all_zero_stamps_are_a_miss() {
        let doc = r#"{"type":"Line","lyrics":[{"time":0,"text":"a"},{"time":0,"text":"b"}]}"#;
        assert!(matches!(decode(payload(doc)), Err(Error::UntimedSync)));
    }

    #[test]
    fn syllables_that_do_not_spell_the_line_fall_back_to_the_line() {
        let doc = r#"{"type":"Word","lyrics":[{"time":1000,"text":"Hello",
            "syllabus":[{"time":1000,"text":"Help"}]}]}"#;
        let parsed = decode(payload(doc)).unwrap();
        assert!(parsed.synced[0].words.is_empty());
        assert_eq!(parsed.synced[0].start, 1.0);
    }
}
