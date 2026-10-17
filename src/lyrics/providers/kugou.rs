//! KuGou: KRC karaoke lyrics (word-timed), falling back to its LRC.
//!
//! Two searches find the track: the song catalogue first, which carries
//! durations, then the lyrics search. Either result has to name this title and
//! artist, because both searches pad their answers with other songs.

use std::io::Read;
use std::sync::LazyLock;
use std::time::Duration;

use base64::Engine;
use regex::Regex;
use serde::Deserialize;
use unicode_script::{Script, UnicodeScript};

use super::Http;
use crate::lyrics::error::{Error, Result};
use crate::lyrics::format::{MAX_LINES, MAX_SOURCE_BYTES, MAX_WORDS, Parser, decode, lrc};
use crate::lyrics::model::{
    Line, Lyrics, Request, Word, is_letter, join_words, milliseconds, sort_lines, trim_words,
};

const SONGS_URL: &str = "https://mobileservice.kugou.com/api/v3/search/song";
const SEARCH_URL: &str = "https://lyrics.kugou.com/search";
const DOWNLOAD_URL: &str = "https://lyrics.kugou.com/download";
/// "Pure music": KuGou's one-line answer for an instrumental.
const INSTRUMENTAL: &str = "纯音乐";
const TIMEOUT: Duration = Duration::from_secs(4);

const KRC_KEY: [u8; 16] = [
    64, 71, 97, 119, 94, 50, 116, 71, 81, 54, 49, 45, 206, 210, 110, 105,
];

static KRC_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([0-9]+),([0-9]+)\](.*)").expect("static regex"));
static KRC_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([0-9]+),([0-9]+),[0-9]+>([^<]*)").expect("static regex"));
static TITLE_SEP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\t\n\x0C\r ]-+[\t\n\x0C\r ]").expect("static regex"));

#[derive(Debug, Default, Deserialize)]
struct Songs {
    #[serde(default)]
    data: Option<SongsData>,
}

#[derive(Debug, Default, Deserialize)]
struct SongsData {
    #[serde(default)]
    info: Option<Vec<Song>>,
}

#[derive(Debug, Default, Deserialize)]
struct Song {
    #[serde(rename = "songname", default)]
    name: Option<String>,
    #[serde(rename = "singername", default)]
    singers: Option<String>,
    #[serde(default)]
    hash: Option<String>,
    #[serde(default)]
    duration: Option<i64>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct Candidate {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "accesskey", default)]
    access_key: Option<String>,
    #[serde(default)]
    song: Option<String>,
    #[serde(rename = "singer", default)]
    singers: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Candidates {
    #[serde(default)]
    candidates: Option<Vec<Candidate>>,
}

#[derive(Debug, Default, Deserialize)]
struct Download {
    #[serde(default)]
    content: Option<String>,
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

pub async fn fetch(http: &Http, req: &Request) -> Result<Lyrics> {
    let candidate = candidate(http, req).await?;
    if let Ok(found) = download(http, req, &candidate, "krc", decode_krc).await
        && !found.is_empty()
    {
        return Ok(found);
    }
    download(http, req, &candidate, "lrc", lrc::parse).await
}

async fn candidate(http: &Http, req: &Request) -> Result<Candidate> {
    let keyword = format!("{} {}", req.title, req.artist);
    let query = pairs(&[
        ("version", "9108"),
        ("plat", "0"),
        ("pagesize", "8"),
        ("showtype", "0"),
        ("keyword", &keyword),
    ]);
    let songs: Songs = http.get_json(SONGS_URL, &query, TIMEOUT).await?;
    let songs = songs.data.and_then(|d| d.info).unwrap_or_default();
    for song in songs {
        if !req.matches_duration(song.duration.unwrap_or(0))
            || !matches(
                req,
                song.name.as_deref().unwrap_or_default(),
                song.singers.as_deref().unwrap_or_default(),
            )
        {
            continue;
        }
        let hash = song.hash.unwrap_or_default();
        match search(http, req, pairs(&[("hash", &hash)])).await {
            Err(e) if e.is_not_found() => continue,
            other => return other,
        }
    }

    let mut query = pairs(&[("keyword", &keyword)]);
    let seconds = req.seconds();
    if seconds > 0 {
        query.push(("duration".into(), (seconds * 1000).to_string()));
    }
    search(http, req, query).await
}

async fn search(http: &Http, req: &Request, mut query: Vec<(String, String)>) -> Result<Candidate> {
    query.extend(pairs(&[("ver", "1"), ("man", "yes"), ("client", "pc")]));
    let payload: Candidates = http.get_json(SEARCH_URL, &query, TIMEOUT).await?;
    payload
        .candidates
        .unwrap_or_default()
        .into_iter()
        .find(|c| {
            matches(
                req,
                c.song.as_deref().unwrap_or_default(),
                c.singers.as_deref().unwrap_or_default(),
            )
        })
        .ok_or(Error::NotFound)
}

fn matches(req: &Request, title: &str, singers: &str) -> bool {
    let artists: Vec<&str> = singers.split('、').collect();
    req.matches_track(title, &artists)
}

async fn download(
    http: &Http,
    req: &Request,
    candidate: &Candidate,
    format: &str,
    parse: Parser,
) -> Result<Lyrics> {
    let query = pairs(&[
        ("ver", "1"),
        ("client", "pc"),
        ("charset", "utf8"),
        ("fmt", format),
        ("id", candidate.id.as_deref().unwrap_or_default()),
        (
            "accesskey",
            candidate.access_key.as_deref().unwrap_or_default(),
        ),
    ]);
    let payload: Download = http.get_json(DOWNLOAD_URL, &query, TIMEOUT).await?;
    let content: String = payload
        .content
        .unwrap_or_default()
        .chars()
        .filter(|c| *c != '\r' && *c != '\n')
        .collect();
    let data = base64::engine::general_purpose::STANDARD
        .decode(content)
        .map_err(Error::invalid)?;
    Ok(tidy(req, parse(&data)?))
}

/// KRC: `krc1`, then zlib data XORed with a fixed key.
pub(crate) fn decode_krc(data: &[u8]) -> Result<Lyrics> {
    let body = data
        .strip_prefix(b"krc1")
        .ok_or_else(|| Error::invalid("invalid krc document"))?;
    let compressed: Vec<u8> = body
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ KRC_KEY[i % KRC_KEY.len()])
        .collect();
    let mut text = Vec::new();
    flate2::read::ZlibDecoder::new(compressed.as_slice())
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut text)
        .map_err(Error::invalid)?;
    if text.len() > MAX_SOURCE_BYTES {
        return Err(Error::TooLarge);
    }
    parse_krc(&text)
}

pub(crate) fn parse_krc(data: &[u8]) -> Result<Lyrics> {
    let text = decode(data)?;
    let mut lines = Vec::new();
    let mut plain = Vec::new();
    let mut words = 0;
    for raw in text.split_inclusive('\n') {
        let Some(found) = KRC_LINE.captures(raw.trim_end_matches(['\r', '\n'])) else {
            continue;
        };
        let Some((start, end)) = span(&found[1], &found[2]) else {
            continue;
        };
        let mut line = Line {
            start,
            end,
            ..Default::default()
        };
        for word in KRC_WORD.captures_iter(&found[3]) {
            let Some((from, to)) = span(&word[1], &word[2]) else {
                continue;
            };
            line.words.push(Word {
                start: start + from,
                end: start + to,
                text: word[3].to_string(),
            });
        }
        line.words = trim_words(line.words);
        line.text = join_words(&line.words);
        if line.text.is_empty() {
            continue;
        }
        words += line.words.len();
        if lines.len() >= MAX_LINES || words > MAX_WORDS {
            return Err(Error::TooLarge);
        }
        plain.push(line.text.clone());
        lines.push(line);
    }
    sort_lines(&mut lines);
    Ok(Lyrics {
        synced: lines,
        plain: plain.join("\n"),
        ..Default::default()
    })
}

/// `offset,length` in milliseconds, as a start and an end.
fn span(offset: &str, length: &str) -> Option<(f64, f64)> {
    let start = milliseconds(offset.parse().ok()?);
    let length = milliseconds(length.parse().ok()?);
    Some((start, start + length))
}

/// Drop the `title - artist` line and the `role：name` credits KuGou puts
/// before the lyrics. Either may be missing.
pub(crate) fn tidy(req: &Request, parsed: Lyrics) -> Lyrics {
    let lines = parsed.synced.clone();
    if lines.is_empty() {
        return parsed;
    }
    if lines.len() == 1 && lines[0].text.contains(INSTRUMENTAL) {
        return Lyrics {
            instrumental: true,
            ..Default::default()
        };
    }
    let mut credits = 0;
    while credits < lines.len() && header_line(&lines[credits].text, credits == 0) {
        credits += 1;
    }
    let lyrics = drop_baked_translations(req, lines[credits..].to_vec());
    let plain = lyrics
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Lyrics {
        synced: lyrics,
        plain,
        ..Default::default()
    }
}

fn header_line(text: &str, first: bool) -> bool {
    if text.contains([':', '：']) {
        return true;
    }
    first && TITLE_SEP.is_match(text)
}

/// Some uploads carry a Chinese translation as a timed line after every
/// original line. Only that strict alternation of Han-only lines is dropped:
/// mixed-language lyrics have CJK lines in a row, a CJK song asked for by a
/// romanised name starts with one, and Japanese or Korean carry kana or Hangul.
fn drop_baked_translations(req: &Request, lines: Vec<Line>) -> Vec<Line> {
    if has_cjk(&format!("{}{}", req.title, req.artist)) {
        return lines;
    }
    let (mut originals, mut translations) = (0, 0);
    for (i, line) in lines.iter().enumerate() {
        if !han_only(&line.text) {
            originals += 1;
        } else if i > 0 && !han_only(&lines[i - 1].text) {
            translations += 1;
        } else {
            return lines;
        }
    }
    if translations == 0 || translations * 5 < originals * 4 {
        return lines;
    }
    lines.into_iter().filter(|l| !han_only(&l.text)).collect()
}

pub(crate) fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c.script(),
            Script::Han | Script::Hiragana | Script::Katakana | Script::Hangul
        )
    })
}

fn han_only(text: &str) -> bool {
    let mut han = false;
    for c in text.chars() {
        if c.script() == Script::Han {
            han = true;
        } else if is_letter(c) {
            return false;
        }
    }
    han
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn krc_lines_and_words() {
        let doc = "[id:$00000000]\n[1000,2000]<0,500,0>Hel<500,500,0>lo <1000,1000,0>you\n";
        let parsed = parse_krc(doc.as_bytes()).unwrap();
        let line = &parsed.synced[0];
        assert_eq!(line.text, "Hello you");
        assert_eq!(line.start, 1.0);
        assert_eq!(line.words[1].start, 1.5);
        assert_eq!(line.words[2].end, 3.0);
    }

    #[test]
    fn krc_round_trips_through_the_cipher() {
        use std::io::Write;
        let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        zlib.write_all(b"[0,1000]<0,1000,0>hi").unwrap();
        let compressed = zlib.finish().unwrap();
        let mut data = b"krc1".to_vec();
        data.extend(
            compressed
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ KRC_KEY[i % 16]),
        );
        assert_eq!(decode_krc(&data).unwrap().synced[0].text, "hi");
    }

    #[test]
    fn credits_and_baked_translations_go() {
        let req = Request {
            title: "Song".into(),
            artist: "Band".into(),
            ..Default::default()
        };
        let line = |t: f64, text: &str| Line {
            start: t,
            text: text.into(),
            ..Default::default()
        };
        let parsed = Lyrics {
            synced: vec![
                line(0.0, "Band - Song"),
                line(1.0, "作词：someone"),
                line(2.0, "hello"),
                line(3.0, "你好"),
                line(4.0, "world"),
                line(5.0, "世界"),
            ],
            ..Default::default()
        };
        let texts: Vec<_> = tidy(&req, parsed)
            .synced
            .into_iter()
            .map(|l| l.text)
            .collect();
        assert_eq!(texts, ["hello", "world"]);
    }

    #[test]
    fn instrumental_marker() {
        let parsed = Lyrics {
            synced: vec![Line {
                text: "纯音乐，请欣赏".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(tidy(&Request::default(), parsed).instrumental);
    }
}
