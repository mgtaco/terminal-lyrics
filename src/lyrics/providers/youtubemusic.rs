//! YouTube Music: line-timed lyrics through the same internal API its own
//! clients use. Search for the song, find its lyrics tab, read the rows.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_script::{Script, UnicodeScript};

use super::{Http, decode_json};
use crate::lyrics::error::{Error, Result};
use crate::lyrics::format::MAX_LINES;
use crate::lyrics::model::{Line, Lyrics, Request, milliseconds, sort_lines};

const API: &str = "https://music.youtube.com/youtubei/v1/";
const SONGS_ONLY: &str = "EgWKAQIIAWoMEA4QChADEAQQCRAF";
const LYRICS_TAB: &str = "MUSIC_PAGE_TYPE_TRACK_LYRICS";

const SONGS: &str = "contents.tabbedSearchResultsRenderer.tabs.tabRenderer.content.sectionListRenderer.contents.musicShelfRenderer.contents.musicResponsiveListItemRenderer";
const TABS: &str = "contents.singleColumnMusicWatchNextResultsRenderer.tabbedRenderer.watchNextTabbedResultsRenderer.tabs.tabRenderer";
const ROWS: &str = "contents.elementRenderer.newElement.type.componentType.model.timedLyricsModel.lyricsData.timedLyricsData";

const COLUMNS: &str = "flexColumns.musicResponsiveListItemFlexColumnRenderer";
const BROWSE: &str = "endpoint.browseEndpoint";

#[derive(Serialize, Clone)]
struct Client {
    #[serde(rename = "clientName")]
    name: String,
    #[serde(rename = "clientVersion")]
    version: String,
    #[serde(rename = "hl")]
    language: String,
}

#[derive(Serialize, Clone)]
struct Context {
    client: Client,
}

#[derive(Serialize)]
struct Body {
    context: Context,
    #[serde(skip_serializing_if = "String::is_empty")]
    query: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    params: String,
    #[serde(rename = "videoId", skip_serializing_if = "String::is_empty")]
    video_id: String,
    #[serde(rename = "browseId", skip_serializing_if = "String::is_empty")]
    browse_id: String,
}

impl Body {
    fn new(context: &Context) -> Self {
        Body {
            context: context.clone(),
            query: String::new(),
            params: String::new(),
            video_id: String::new(),
            browse_id: String::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Row {
    #[serde(rename = "lyricLine", default)]
    text: Option<String>,
    #[serde(rename = "cueRange", default)]
    cue: Option<Cue>,
}

#[derive(Debug, Default, Deserialize)]
struct Cue {
    #[serde(rename = "startTimeMilliseconds", default)]
    start: Option<String>,
    #[serde(rename = "endTimeMilliseconds", default)]
    end: Option<String>,
}

pub async fn fetch(http: &Http, req: &Request) -> Result<Lyrics> {
    let web = Context {
        client: Client {
            name: "WEB_REMIX".into(),
            version: format!("1.{}.01.00", utc_date()),
            language: language(req).into(),
        },
    };

    let mut body = Body::new(&web);
    body.query = format!("{} {}", req.title, req.artist);
    body.params = SONGS_ONLY.into();
    let fields = format!("{SONGS}(playlistItemData.videoId,{COLUMNS}.text.runs.text)");
    let search = call(http, "search", &fields, &body).await?;
    let video_id = video_id(&search, req).ok_or(Error::NotFound)?;

    let mut body = Body::new(&web);
    body.video_id = video_id;
    let fields =
        format!("{TABS}(unselectable,{BROWSE}(browseId,browseEndpointContextSupportedConfigs))");
    let next = call(http, "next", &fields, &body).await?;
    let lyrics_id = lyrics_id(&next).ok_or(Error::NotFound)?;

    // Only the mobile client gets timed lyrics.
    let mobile = Context {
        client: Client {
            name: "ANDROID_MUSIC".into(),
            version: "7.21.50".into(),
            language: "en".into(),
        },
    };
    let mut body = Body::new(&mobile);
    body.browse_id = lyrics_id;
    let page = call(http, "browse", ROWS, &body).await?;
    let found = dig(&page, ROWS);
    let first = found.first().ok_or(Error::NotFound)?;
    let rows: Vec<Row> = serde_json::from_value((*first).clone()).map_err(Error::invalid)?;
    decode(rows)
}

async fn call(http: &Http, endpoint: &str, fields: &str, body: &Body) -> Result<Value> {
    let request = http
        .client()
        .post(format!("{API}{endpoint}?prettyPrint=false"))
        .timeout(Duration::from_secs(4))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        // The lyrics page alone is over the size cap without a field mask.
        .header("X-Goog-FieldMask", fields)
        .body(serde_json::to_vec(body).map_err(Error::invalid)?);
    decode_json(&http.send(request).await?)
}

fn utc_date() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}")
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Search localises names: with `hl=en`, 米津玄師 comes back as Kenshi Yonezu.
fn language(req: &Request) -> &'static str {
    for c in format!("{}{}", req.artist, req.title).chars() {
        match c.script() {
            Script::Hangul => return "ko",
            Script::Han | Script::Hiragana | Script::Katakana => return "ja",
            _ => {}
        }
    }
    "en"
}

fn video_id(search: &Value, req: &Request) -> Option<String> {
    for song in dig(search, SONGS) {
        let columns = dig(song, COLUMNS);
        if columns.len() < 2 {
            continue;
        }
        let title = dig_strings(columns[0], "text.runs.text").concat();
        let details = dig_strings(columns[1], "text.runs.text");
        let video_id = dig_string(song, "playlistItemData.videoId");
        let artists: Vec<&str> = artists(&details).iter().map(String::as_str).collect();
        if !video_id.is_empty()
            && req.matches_duration(seconds(&details))
            && req.matches_track(&title, &artists)
        {
            return Some(video_id);
        }
    }
    None
}

/// A song's details read `Kygo`, ` & `, `Zak Abel`, ` • `, album, ` • `, `3:19`.
fn artists(details: &[String]) -> &[String] {
    match details.iter().position(|d| d == " • ") {
        Some(end) => &details[..end],
        None => details,
    }
}

fn seconds(details: &[String]) -> i64 {
    details.last().and_then(|d| clock_seconds(d)).unwrap_or(0)
}

fn lyrics_id(next: &Value) -> Option<String> {
    let page_type = format!(
        "{BROWSE}.browseEndpointContextSupportedConfigs.browseEndpointContextMusicConfig.pageType"
    );
    dig(next, TABS)
        .into_iter()
        .find(|tab| {
            dig_string(tab, &page_type) == LYRICS_TAB && dig(tab, "unselectable").is_empty()
        })
        .map(|tab| dig_string(tab, &format!("{BROWSE}.browseId")))
}

fn clock_seconds(text: &str) -> Option<i64> {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut total = 0;
    for part in parts {
        let value: i64 = part.parse().ok().filter(|v| *v >= 0)?;
        total = total * 60 + value;
    }
    Some(total)
}

pub(crate) fn decode(rows: Vec<Row>) -> Result<Lyrics> {
    if rows.len() > MAX_LINES {
        return Err(Error::TooLarge);
    }
    let plain: Vec<String> = rows
        .iter()
        .map(|r| r.text.clone().unwrap_or_default())
        .collect();
    let mut synced: Vec<Line> = rows.iter().filter_map(Row::line).collect();
    let mut result = Lyrics {
        plain: plain.join("\n"),
        ..Default::default()
    };
    // All or nothing: a row without timing would leave a hole in the song.
    if synced.len() == rows.len() {
        sort_lines(&mut synced);
        result.synced = synced;
    }
    Ok(result)
}

impl Row {
    fn line(&self) -> Option<Line> {
        let cue = self.cue.as_ref()?;
        let start: i64 = cue.start.as_deref()?.parse().ok()?;
        let end: i64 = cue.end.as_deref()?.parse().ok()?;
        Some(Line {
            start: milliseconds(start),
            end: milliseconds(end),
            text: self.text.clone().unwrap_or_default(),
            ..Default::default()
        })
    }
}

/// Read a path the way a field mask does, stepping into every element of an
/// array on the way.
fn dig<'a>(data: &'a Value, path: &str) -> Vec<&'a Value> {
    let mut values = vec![data];
    for key in path.split('.') {
        let mut next = Vec::new();
        for value in values {
            let items: Vec<&Value> = match value {
                Value::Array(items) => items.iter().collect(),
                other => vec![other],
            };
            for item in items {
                if let Some(found) = item.as_object().and_then(|o| o.get(key)) {
                    next.push(found);
                }
            }
        }
        values = next;
    }
    values
}

fn dig_strings(data: &Value, path: &str) -> Vec<String> {
    dig(data, path)
        .into_iter()
        .filter_map(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Null => Some(String::new()),
            _ => None,
        })
        .collect()
}

fn dig_string(data: &Value, path: &str) -> String {
    dig_strings(data, path)
        .into_iter()
        .next()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_from_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_743), (2026, 10, 17));
    }

    #[test]
    fn finds_the_matching_song() {
        let search = serde_json::json!({"contents":{"tabbedSearchResultsRenderer":{"tabs":[{"tabRenderer":{"content":{"sectionListRenderer":{"contents":[{"musicShelfRenderer":{"contents":[
            {"musicResponsiveListItemRenderer":{"playlistItemData":{"videoId":"wrong"},"flexColumns":[
                {"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"Get Lucky"}]}}},
                {"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"Daft Punk"},{"text":" • "},{"text":"RAM"},{"text":" • "},{"text":"6:09"}]}}}]}},
            {"musicResponsiveListItemRenderer":{"playlistItemData":{"videoId":"right"},"flexColumns":[
                {"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"Get Lucky"}]}}},
                {"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"Daft Punk"},{"text":" & "},{"text":"Pharrell Williams"},{"text":" • "},{"text":"RAM"},{"text":" • "},{"text":"4:08"}]}}}]}}
        ]}}]}}}}]}}});
        let req = Request {
            title: "Get Lucky".into(),
            artist: "Daft Punk".into(),
            duration: 248.0,
            ..Default::default()
        };
        assert_eq!(video_id(&search, &req).as_deref(), Some("right"));
    }

    #[test]
    fn a_row_without_timing_unsyncs_the_song() {
        let rows: Vec<Row> = serde_json::from_str(
            r#"[{"lyricLine":"a","cueRange":{"startTimeMilliseconds":"1000","endTimeMilliseconds":"2000"}},{"lyricLine":"b"}]"#,
        )
        .unwrap();
        let parsed = decode(rows).unwrap();
        assert!(parsed.synced.is_empty());
        assert_eq!(parsed.plain, "a\nb");
    }
}
