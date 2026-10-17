//! The lyrics model the providers answer in, and the track they are asked about.
//!
//! This is DankMaterialShell's model (`dankgo/lyrics`), field for field, and it
//! serialises to the same JSON — the cache files are that JSON, and the cache is
//! shared with DMS (see [`super::cache`]). Display never reads this directly:
//! [`super::display`] turns it into [`crate::lrc::Lyrics`] once, at the end.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

/// Seconds from the start of the track.
pub type Seconds = f64;

pub(crate) fn milliseconds(ms: i64) -> Seconds {
    ms as f64 / 1000.0
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

fn is_zero_i(v: &i64) -> bool {
    *v == 0
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// Go writes a nil slice as `null`; read that as empty.
fn nullable<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// One timed span. Concatenating a line's word texts gives the line's text
/// exactly, spaces included.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Word {
    #[serde(rename = "t", default)]
    pub start: Seconds,
    /// 0 when the source did not say.
    #[serde(rename = "e", default, skip_serializing_if = "is_zero")]
    pub end: Seconds,
    #[serde(rename = "x", default, deserialize_with = "nullable")]
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Line {
    #[serde(rename = "t", default)]
    pub start: Seconds,
    /// 0 when the source did not say.
    #[serde(rename = "e", default, skip_serializing_if = "is_zero")]
    pub end: Seconds,
    #[serde(rename = "x", default, deserialize_with = "nullable")]
    pub text: String,
    /// Only set for word-synced sources.
    #[serde(
        rename = "w",
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub words: Vec<Word>,
    /// A key into [`Lyrics::voices`].
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "String::is_empty"
    )]
    pub voice: String,
    /// A backing vocal.
    #[serde(default, skip_serializing_if = "is_false")]
    pub background: bool,
    /// Shared by the lines split out of one source line — a lead and the
    /// backing vocal under it. 0 means not grouped.
    #[serde(default, skip_serializing_if = "is_zero_i")]
    pub group: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// `person` or `group`, or empty.
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
}

/// Which parser produced a [`Lyrics`]. Not serialised.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SourceFormat {
    #[default]
    Unknown,
    Lrc,
    Ttml,
    Lyricsfile,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lyrics {
    /// Sorted by start; empty when the source has no timing.
    #[serde(default, deserialize_with = "nullable")]
    pub synced: Vec<Line>,
    #[serde(default, deserialize_with = "nullable")]
    pub plain: String,
    #[serde(default)]
    pub instrumental: bool,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub voices: BTreeMap<String, Voice>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "String::is_empty"
    )]
    pub title: String,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "String::is_empty"
    )]
    pub artist: String,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "String::is_empty"
    )]
    pub album: String,
    #[serde(default, skip_serializing_if = "is_zero_i")]
    pub duration_ms: i64,
    #[serde(skip)]
    pub source: SourceFormat,
}

impl Lyrics {
    /// No lyrics at all: not instrumental, nothing timed, no plain text.
    pub fn is_empty(&self) -> bool {
        !self.instrumental && self.synced.is_empty() && self.plain.trim().is_empty()
    }

    pub fn word_synced(&self) -> bool {
        self.synced.iter().any(|l| !l.words.is_empty())
    }

    /// 2 for word sync, 1 for line sync or an instrumental, 0 for plain text.
    pub fn tier(&self) -> u8 {
        if self.word_synced() {
            2
        } else if !self.synced.is_empty() || self.instrumental {
            1
        } else {
            0
        }
    }

    /// Whether any stamp at all is non-zero.
    pub(crate) fn timed(&self) -> bool {
        self.synced.iter().any(|l| {
            l.start > 0.0 || l.end > 0.0 || l.words.iter().any(|w| w.start > 0.0 || w.end > 0.0)
        })
    }
}

pub(crate) fn sort_lines(lines: &mut [Line]) {
    // Stable, so lines that start together keep their source order.
    lines.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

pub(crate) fn join_words(words: &[Word]) -> String {
    words.iter().map(|w| w.text.as_str()).collect()
}

/// Trim whitespace off the first and last word, dropping words that were
/// nothing else.
pub(crate) fn trim_words(mut words: Vec<Word>) -> Vec<Word> {
    while let Some(first) = words.first_mut() {
        first.text = first.text.trim_start().to_string();
        if !first.text.is_empty() {
            break;
        }
        words.remove(0);
    }
    while let Some(last) = words.last_mut() {
        last.text = last.text.trim_end().to_string();
        if !last.text.is_empty() {
            break;
        }
        words.pop();
    }
    words
}

pub(crate) fn bounded_voice_id(value: &str) -> String {
    if value.len() > 128 {
        String::new()
    } else {
        value.to_string()
    }
}

pub(crate) fn voice_metadata(name: &str, kind: &str) -> Voice {
    let name = if name.len() > 512 { "" } else { name };
    let kind = match kind {
        "person" | "group" => kind,
        _ => "",
    };
    Voice {
        name: name.to_string(),
        kind: kind.to_string(),
    }
}

/// What to look up. Artist and title are required; the rest narrows the match.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Track length in seconds; 0 is unknown.
    pub duration: f64,
    /// The track's `file://` URL. Turns on the lookup next to the file.
    pub file_url: String,
    /// Never touch the network; local files and cached answers only.
    pub cache_only: bool,
}

/// How far a catalogue's length may be from the track's and still be the same
/// recording.
const DURATION_SLACK: i64 = 8;

impl Request {
    /// Whole seconds, rounded the way DMS rounds them (half up).
    pub fn seconds(&self) -> i64 {
        if self.duration.is_finite() && self.duration > 0.0 {
            (self.duration + 0.5).floor() as i64
        } else {
            0
        }
    }

    pub(crate) fn matches_duration(&self, seconds: i64) -> bool {
        self.duration_gap(seconds) <= DURATION_SLACK
    }

    /// 0 when the request has no duration.
    pub(crate) fn duration_gap(&self, seconds: i64) -> i64 {
        let want = self.seconds();
        if want == 0 {
            return 0;
        }
        (want - seconds).abs()
    }

    /// Whether a search result is this track. Searches pad their results with
    /// other songs by the artist and with other people's uploads.
    pub(crate) fn matches_track(&self, title: &str, artists: &[&str]) -> bool {
        let name = searchable(title);
        if name.is_empty() || !contains_words(&searchable(&self.title), &name) {
            return false;
        }
        let known = searchable(&format!("{} {}", self.artist, self.title));
        artists.iter().any(|artist| {
            let artist = searchable(artist);
            !artist.is_empty() && contains_words(&known, &artist)
        })
    }

    /// Query parameters under the names a provider uses for them; album and
    /// duration only when known.
    pub(crate) fn query(
        &self,
        title: &str,
        artist: &str,
        album: &str,
        duration: &str,
    ) -> Vec<(String, String)> {
        let mut query = vec![
            (title.to_string(), self.title.clone()),
            (artist.to_string(), self.artist.clone()),
        ];
        if !self.album.is_empty() {
            query.push((album.to_string(), self.album.clone()));
        }
        let seconds = self.seconds();
        if seconds > 0 {
            query.push((duration.to_string(), seconds.to_string()));
        }
        query
    }
}

/// Lowercase one char at a time, the way Go's `strings.ToLower` does: no
/// context-dependent final sigma, and `İ` is plain `i`. Cache keys are built
/// from this, so it has to agree with DMS exactly.
pub(crate) fn go_lower(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ if c == '\u{130}' => 'i',
                _ => c,
            }
        })
        .collect()
}

/// Unicode general category L, which is what Go's `unicode.IsLetter` tests.
/// `char::is_alphabetic` is wider: it takes in combining vowel signs.
pub(crate) fn is_letter(c: char) -> bool {
    use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};
    c.general_category_group() == GeneralCategoryGroup::Letter
}

/// Notes like `(Radio Edit)`, in any of the bracket styles catalogues use.
fn strip_bracketed(text: &str) -> String {
    const OPEN: [char; 4] = ['(', '[', '（', '【'];
    const CLOSE: [char; 4] = [')', ']', '）', '】'];
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if OPEN.contains(&chars[i])
            && let Some(len) = chars[i + 1..].iter().position(|c| CLOSE.contains(c))
        {
            out.push(' ');
            i += len + 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Lowercased words, with bracketed notes, quotes and punctuation dropped.
pub(crate) fn searchable(text: &str) -> String {
    strip_bracketed(&go_lower(text))
        .split(|c: char| !is_letter(c) && !c.is_numeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn contains_words(text: &str, words: &str) -> bool {
    format!(" {text} ").contains(&format!(" {words} "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brackets_and_punctuation_drop_out() {
        assert_eq!(
            searchable("Get Lucky (Radio Edit) [feat. Pharrell]"),
            "get lucky"
        );
        assert_eq!(searchable("Don't Stop"), "don t stop");
        assert_eq!(searchable("【MV】夜に駆ける"), "夜に駆ける");
    }

    #[test]
    fn track_match_wants_title_words_and_an_artist() {
        let req = Request {
            title: "Get Lucky (Radio Edit)".into(),
            artist: "Daft Punk".into(),
            ..Default::default()
        };
        assert!(req.matches_track("Get Lucky", &["Daft Punk", "Pharrell Williams"]));
        assert!(!req.matches_track("Lucky", &["Someone"]));
        assert!(!req.matches_track("Get Lucky Remix", &["Daft Punk"]));
    }

    #[test]
    fn seconds_round_half_up() {
        let at = |d| Request {
            duration: d,
            ..Default::default()
        };
        assert_eq!(at(368.5).seconds(), 369);
        assert_eq!(at(368.4).seconds(), 368);
        assert_eq!(at(0.0).seconds(), 0);
    }
}
