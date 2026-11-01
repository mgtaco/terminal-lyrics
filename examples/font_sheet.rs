//! Prints every font's glyphs, for checking the art in `src/render/fonts/`.
//! `cargo run --example font_sheet [block|compact|mini] [text…]`.

use terminal_lyrics::render::{Screen, Theme, font, render};

const SAMPLES: &[&str] = &[
    "ABCDEFGHIJKLM",
    "NOPQRSTUVWXYZ",
    "abcdefghijklm",
    "nopqrstuvwxyz",
    "0123456789",
    "'\"‘’“”`,.…!¡?¿-–—_",
    "()[]:;/\\&#*+=<>@$%~^«»♪♫",
    "ÆæŒœØøŁłĐđÐðĦħÞþßẞŊŋə",
    "ÀÁÂÃÄÅĀĂĄÇČĎÉĚËĘĞĢİÍÑŇŐŘŚŠŞȘŢŤÚŮŰÝŸŹŻŽ",
    "àáâãäåāăąçčďéěëęğģíïñňőřśšşșţťúůűýÿźżž",
    "Tiếng Việt ơ ư ạ ặ ổ",
    "Ángel jugó señor",
    "АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ",
    "абвгдеёжзийклмнопрстуфхцчшщъыьэюя",
    "ІЇЄҐЎ іїєґў",
    "君の名は「希望」、カタカナ！",
    "한국어 가사 中文歌词",
];

fn main() {
    let mut args = std::env::args().skip(1);
    let fonts: Vec<_> = match args.next() {
        Some(name) => vec![font::by_name(&name).expect("block, compact or mini")],
        None => vec![font::block(), font::compact()],
    };
    let rest: Vec<String> = args.collect();
    let texts: Vec<&str> = match rest.is_empty() {
        true => SAMPLES.to_vec(),
        false => rest.iter().map(String::as_str).collect(),
    };
    let width = terminal_width();
    for f in &fonts {
        println!("── {} ──", f.name);
        for text in &texts {
            let screen = Screen::Lyric {
                text,
                highlight: 0,
                reveal: usize::MAX,
                second: None,
            };
            let out = render(&screen, f, width, 200, Theme::default());
            let rows: Vec<String> = out
                .lines
                .iter()
                .map(|l| {
                    l.spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                })
                .map(|r| r.trim_end().to_string())
                .skip_while(|r| r.is_empty())
                .collect();
            for row in &rows {
                println!("{row}");
            }
            println!();
        }
    }
}

fn terminal_width() -> u16 {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(160)
}
