use std::time::Duration;

use terminal_lyrics::render::font;
use terminal_lyrics::render::title::{PER_CHAR, status_row, typewriter};
use terminal_lyrics::render::{HelpRow, Screen, Theme, help, render};
use unicode_width::UnicodeWidthStr;

const OLD: &str = "Daft Punk — Digital Love";
const NEW: &str = "Radiohead — Karma Police";

fn chars(n: u32) -> Duration {
    PER_CHAR * n
}

#[test]
fn the_new_name_types_over_the_old_from_the_left() {
    assert_eq!(typewriter(OLD, NEW, chars(0)), OLD);
    assert_eq!(typewriter(OLD, NEW, chars(3)), "Radt Punk — Digital Love");
    assert_eq!(typewriter(OLD, NEW, chars(24)), NEW);
    assert_eq!(typewriter(OLD, NEW, chars(100)), NEW);
}

#[test]
fn a_longer_name_types_in_ahead_of_the_old_one() {
    assert_eq!(typewriter("Short", "Longer name", chars(0)), "      Short");
    assert_eq!(typewriter("Short", "Longer name", chars(3)), "Lon   Short");
    assert_eq!(typewriter("Short", "Longer name", chars(7)), "Longer hort");
    assert_eq!(typewriter("Short", "Longer name", chars(11)), "Longer name");
}

#[test]
fn a_shorter_name_wipes_the_old_one_from_the_left() {
    assert_eq!(typewriter("Longer name", "Short", chars(3)), "   ger name");
    assert_eq!(typewriter("Longer name", "Short", chars(8)), "      Shame");
    assert_eq!(typewriter("Longer name", "Short", chars(11)), "Short");
}

#[test]
fn the_old_name_never_moves() {
    for (from, to) in [
        ("Short", NEW),
        (NEW, "Short"),
        ("", NEW),
        ("宇多田ヒカル — First Love", NEW),
        (OLD, "宇多田ヒカル — First Love"),
    ] {
        let width = from.width().max(to.width());
        for n in 0..40 {
            let s = typewriter(from, to, chars(n));
            if s != to {
                assert_eq!(s.width(), width, "{from} → {to} after {n}: {s:?}");
            }
        }
    }
    assert_eq!(typewriter("", "Song", chars(2)), "So  ");
    assert_eq!(typewriter("", "", chars(0)), "");
}

#[test]
fn the_status_row_puts_the_name_on_the_right() {
    let row = status_row("lrclib", "Song", 20);
    assert_eq!(row, " lrclib        Song ");
    assert_eq!(row.width(), 20);
    assert_eq!(status_row("", "Song", 10), "     Song ");
    assert_eq!(status_row("lrclib", "", 20), " lrclib");
    assert_eq!(status_row("", "", 20), "");
}

#[test]
fn the_name_gives_way_when_the_row_is_too_narrow() {
    let row = status_row("lrclib", OLD, 20);
    assert_eq!(row, " lrclib  Daft Punk… ");
    assert_eq!(row.width(), 20);
    assert_eq!(status_row("lrclib", "Song", 9), " lrclib");
    let wide = status_row("", "宇多田ヒカル", 9);
    assert!(wide.width() <= 9, "{wide}");
    assert!(wide.ends_with("… "), "{wide}");
}

#[test]
fn the_key_overlay_lines_up_its_columns() {
    let rows = [
        HelpRow {
            keys: "f",
            action: "font",
            value: "block".into(),
        },
        HelpRow {
            keys: "",
            action: "",
            value: String::new(),
        },
        HelpRow {
            keys: "space",
            action: "play / pause",
            value: String::new(),
        },
    ];
    let text = help(&rows, 60, 20, Theme::default());
    let lines: Vec<String> = text.lines.iter().map(|l| l.to_string()).collect();
    let font = lines.iter().find(|l| l.contains("font")).unwrap();
    let play = lines.iter().find(|l| l.contains("play")).unwrap();
    assert_eq!(font.find("font"), play.find("play"), "{font:?} / {play:?}");
    assert!(font.trim_end().ends_with("block"));
    assert!(lines.iter().any(|l| l.contains("? or Esc to close")));
}

fn dots(elapsed: f64, total: f64, font: &font::Font) -> Vec<String> {
    let text = render(
        &Screen::Waiting { elapsed, total },
        font,
        60,
        20,
        Theme::default(),
    );
    text.lines
        .iter()
        .map(|l| l.to_string())
        .filter(|l| !l.trim().is_empty())
        .collect()
}

#[test]
fn the_dots_grow_one_after_another() {
    let block = font::block();
    let early = dots(1.0, 30.0, &block);
    assert_eq!(early.len(), 2, "two rows of dots");
    assert_eq!(early[0].matches(" ▄▄ ").count(), 1, "{early:?}");
    assert_eq!(
        early[0].matches(" \u{1CEA0}\u{1CEA3} ").count(),
        2,
        "{early:?}"
    );
    let width = |rows: &[String]| rows.iter().map(|r| r.width()).collect::<Vec<_>>();

    let late = dots(25.0, 30.0, &block);
    assert_eq!(late[0].matches("▄██▄").count(), 2, "{late:?}");
    assert_eq!(late[0].matches(" ▄▄ ").count(), 1, "{late:?}");
    assert_eq!(
        width(&early),
        width(&late),
        "the dots stay put as they grow"
    );

    let full = dots(29.7, 30.0, &block);
    assert_eq!(full[0].matches("▄██▄").count(), 3, "the last is seen full");
}

#[test]
fn the_dots_appear_and_stay_full_to_the_end() {
    let block = font::block();
    assert!(dots(0.0, 30.0, &block).is_empty());
    assert_eq!(dots(29.75, 30.0, &block)[0].matches("▄██▄").count(), 3);
    let mini = dots(15.0, 30.0, &font::mini());
    assert_eq!(mini.len(), 1);
    assert_eq!(mini[0].matches('●').count(), 1, "{mini:?}");
    assert_eq!(mini[0].matches('•').count(), 1, "{mini:?}");
    assert_eq!(mini[0].matches('·').count(), 1, "{mini:?}");
}
