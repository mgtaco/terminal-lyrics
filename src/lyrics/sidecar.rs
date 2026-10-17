//! Lyrics files sitting next to the track itself.
//!
//! For `file:///music/get-lucky.flac` the whole search is, in order:
//! `/music/get-lucky.lyricsfile.yaml`, `.ttml`, `.lrc`, `.LRC`. There is no
//! library scan. The first that parses to something wins, beats every
//! provider, and needs no network. MPRIS players report the URL as
//! `xesam:url`.

use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;

use super::format::{MAX_SIDECAR_BYTES, Parser, lrc, lyricsfile, ttml};
use super::model::Lyrics;

const FORMATS: [(&str, Parser); 4] = [
    (".lyricsfile.yaml", lyricsfile::parse),
    (".ttml", ttml::parse),
    (".lrc", lrc::parse),
    (".LRC", lrc::parse),
];

/// The first sidecar next to `file_url` with lyrics in it.
pub fn find(file_url: &str) -> Option<(PathBuf, Lyrics)> {
    let path = local_path(file_url)?;
    let mut base = OsString::from(path.with_extension("").as_os_str());
    if path.extension().is_none() {
        base = path.clone().into_os_string();
    }
    for (suffix, parse) in FORMATS {
        let mut candidate = base.clone();
        candidate.push(suffix);
        let candidate = PathBuf::from(candidate);
        let Some(data) = read(&candidate) else {
            continue;
        };
        match parse(&data) {
            Ok(parsed) if !parsed.is_empty() => return Some((candidate, parsed)),
            _ => continue,
        }
    }
    None
}

fn local_path(file_url: &str) -> Option<PathBuf> {
    if !file_url.starts_with("file://") {
        return None;
    }
    let url = reqwest::Url::parse(file_url).ok()?;
    match url.host_str() {
        None | Some("") | Some("localhost") => {}
        Some(_) => return None,
    }
    let path = url.to_file_path().ok()?;
    path.is_absolute().then_some(path)
}

fn read(path: &std::path::Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_SIDECAR_BYTES {
        return None;
    }
    let mut data = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_SIDECAR_BYTES + 1)
        .read_to_end(&mut data)
        .ok()?;
    (data.len() as u64 <= MAX_SIDECAR_BYTES).then_some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_lrc_next_to_the_track() {
        let dir = std::env::temp_dir().join(format!("tl-sidecar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("my song.lrc"), "[00:01.00]hello").unwrap();
        let url = reqwest::Url::from_file_path(dir.join("my song.flac")).unwrap();
        let (path, lyrics) = find(url.as_str()).unwrap();
        assert_eq!(path, dir.join("my song.lrc"));
        assert_eq!(lyrics.synced[0].text, "hello");
        assert!(find("https://example.com/song.flac").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
