//! The cava strip: the config handed to cava, its frames, and the bars drawn.

use ratatui::style::Style;
use terminal_lyrics::cava::{MAX_LEVEL, config_text, parse_frame};
use terminal_lyrics::render::bars::{BAR_GAP, BAR_WIDTH, bar_count, render};

fn section<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let header = format!("[{name}]");
    text.lines()
        .skip_while(|l| *l != header)
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.is_empty())
        .collect()
}

#[test]
fn without_a_user_config_cava_gets_our_shape_and_raw_output() {
    let text = config_text(None, 40, 30);
    let general = section(&text, "general");
    assert!(general.contains(&"bars = 40"));
    assert!(general.contains(&"framerate = 30"));
    let output = section(&text, "output");
    assert!(output.contains(&"method = raw"));
    assert!(output.contains(&"data_format = ascii"));
    assert!(output.contains(&"ascii_max_range = 1000"));
    assert_eq!(MAX_LEVEL, 1000);
}

#[test]
fn the_users_input_is_kept_so_the_same_source_is_captured() {
    let user = "\
[general]
; a comment
bars = 100
framerate = 144
sensitivity = 120

[input]
method = pipewire
source = spotify

[output]
method = ncurses
channels = mono

[color]
gradient = 1
";
    let text = config_text(Some(user), 40, 30);
    assert_eq!(
        section(&text, "input"),
        ["method = pipewire", "source = spotify"]
    );
    let general = section(&text, "general");
    assert!(
        general.contains(&"sensitivity = 120"),
        "sound settings stay"
    );
    assert!(!general.contains(&"bars = 100"), "the shape is ours");
    assert!(!general.contains(&"framerate = 144"));
    let output = section(&text, "output");
    assert!(output.contains(&"method = raw"));
    assert!(!output.contains(&"method = ncurses"));
    assert!(
        output.contains(&"channels = mono"),
        "still means something raw"
    );
    assert!(!text.contains("[color]"), "nothing of cava's own drawing");
    assert_eq!(text.matches("[general]").count(), 1);
}

#[test]
fn a_frame_is_one_number_per_bar_and_the_trailing_delimiter_is_not_a_bar() {
    assert_eq!(parse_frame("0;3;12;7;\n"), [0, 3, 12, 7]);
    assert_eq!(parse_frame("5;x;2;"), [5, 0, 2], "garbage reads as silence");
    assert!(parse_frame("").is_empty());
}

#[test]
fn bars_come_in_pairs_for_the_two_channels() {
    for width in 0..200 {
        let n = bar_count(width);
        assert_eq!(n % 2, 0, "width {width}");
        let used = (n * (BAR_WIDTH + BAR_GAP)).saturating_sub(BAR_GAP);
        assert!(used <= width, "width {width}: {n} bars need {used}");
    }
    assert_eq!(bar_count(5), 2);
    assert_eq!(bar_count(4), 0, "not even a pair fits");
}

/// Levels on a scale of eight per row, so each step is one eighth block.
fn rows(levels: &[u16], rows: usize, width: usize) -> Vec<String> {
    render(
        levels,
        rows as u16 * 8,
        rows,
        width,
        Style::default(),
        false,
    )
    .lines
    .iter()
    .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
    .collect()
}

#[test]
fn a_bar_fills_from_the_bottom_in_eighths() {
    // 2 rows: 16 steps. 11 is one full cell and three eighths of the next.
    assert_eq!(rows(&[11, 0], 2, 5), ["▃▃", "██"]);
    assert_eq!(rows(&[16, 8], 2, 5), ["██", "██ ██"]);
    assert_eq!(rows(&[0, 0], 2, 5), ["", ""], "silence draws nothing");
}

#[test]
fn levels_are_fitted_to_however_many_rows_there_are() {
    let half = [500, 0];
    let draw = |rows| render(&half, 1000, rows, 5, Style::default(), false);
    let full = |rows: usize| -> usize {
        draw(rows)
            .lines
            .iter()
            .filter(|l| l.spans.iter().any(|s| s.content.contains('█')))
            .count()
    };
    assert_eq!(full(2), 1, "half of two rows");
    assert_eq!(full(8), 4, "half of eight");
    assert!(
        render(&[1, 1], 1000, 2, 5, Style::default(), false)
            .lines
            .iter()
            .all(|l| l.spans.iter().all(|s| s.content.trim().is_empty())),
        "a whisper of noise draws nothing"
    );
}

#[test]
fn bars_are_centred_and_a_level_past_the_top_is_clipped() {
    let out = rows(&[99, 1], 1, 11);
    assert_eq!(out, ["   ██ ▁▁"]);
}

#[test]
fn a_frame_wider_than_the_window_loses_its_ends_not_its_middle() {
    // Four bars into room for two: the middle pair stays.
    let out = rows(&[8, 1, 2, 8], 1, 5);
    assert_eq!(out, ["▁▁ ▂▂"]);
}

#[test]
fn the_console_gets_only_half_and_full_blocks() {
    let levels: Vec<u16> = (0..=16).collect();
    let text = render(&levels, 16, 2, 80, Style::default(), true);
    let drawn: String = text.lines.iter().map(|l| l.to_string()).collect();
    assert!(
        drawn.chars().all(|c| matches!(c, ' ' | '▄' | '█')),
        "{drawn:?}"
    );
    assert!(drawn.contains('▄') && drawn.contains('█'), "{drawn:?}");
}
