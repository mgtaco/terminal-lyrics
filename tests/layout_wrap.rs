//! Wrapping and centring. The load-bearing property is that no character is
//! ever dropped, however narrow the terminal gets.

use terminal_lyrics::render::layout::{Layout, layout, layout_fitting};
use terminal_lyrics::render::{LINE_GAP, Screen, SecondVoice, Theme, font, render};

/// Every source character that is not whitespace must survive layout.
fn assert_no_loss(text: &str, l: &Layout) {
    let mut got: Vec<usize> = l
        .lines
        .iter()
        .flat_map(|line| line.cells.iter().map(|c| c.src))
        .collect();
    got.sort_unstable();
    got.dedup();

    let want: Vec<usize> = text
        .char_indices()
        .enumerate()
        .filter(|(_, (_, c))| !c.is_whitespace())
        .map(|(i, _)| i)
        .collect();

    for idx in want {
        assert!(
            got.contains(&idx),
            "character {idx} ({:?}) was dropped from {text:?}",
            text.chars().nth(idx).unwrap()
        );
    }
}

#[test]
fn a_short_line_stays_on_one_row_of_glyphs() {
    let f = font::block();
    let l = layout("HI", &f, 100);
    assert_eq!(l.lines.len(), 1);
    assert_eq!(l.height, 5);
    assert_no_loss("HI", &l);
}

#[test]
fn a_long_line_wraps_instead_of_being_cut() {
    let f = font::block();
    let text = "NEVER GONNA GIVE YOU UP NEVER GONNA LET YOU DOWN";
    let l = layout(text, &f, 60);
    assert!(l.lines.len() > 1, "should have wrapped");
    for line in &l.lines {
        assert!(line.width <= 60, "line of width {} exceeds 60", line.width);
    }
    assert_no_loss(text, &l);
}

#[test]
fn wrapping_prefers_word_boundaries() {
    let f = font::mini();
    // "hello world" needs 11 columns; at 10 the two words must separate,
    // and the break must fall between them rather than mid-word.
    let l = layout("hello world", &f, 10);
    assert_eq!(l.lines.len(), 2);
    let first: String = l.lines[0].cells.iter().map(|c| c.rows[0].clone()).collect();
    assert_eq!(first.trim(), "hello");
}

#[test]
fn a_word_wider_than_the_terminal_is_broken_not_discarded() {
    let f = font::block();
    let text = "SUPERCALIFRAGILISTIC";
    let l = layout(text, &f, 30);
    assert!(l.lines.len() > 1);
    for line in &l.lines {
        assert!(line.width <= 30);
    }
    assert_no_loss(text, &l);
}

#[test]
fn source_indices_survive_wrapping_so_the_sweep_stays_aligned() {
    let f = font::mini();
    let text = "one two three";
    let l = layout(text, &f, 7);
    // The 't' of "three" is at index 8 in the source; whichever visual line it
    // lands on, it must still say 8, or the highlight would jump.
    let found = l
        .lines
        .iter()
        .flat_map(|line| line.cells.iter())
        .find(|c| c.src == 8);
    assert!(found.is_some());
    assert_no_loss(text, &l);
}

#[test]
fn the_font_steps_down_before_anything_is_lost() {
    // Five rows of block art cannot fit in four; the fitter must pick a
    // shorter font rather than clip the glyphs.
    let block = font::block();
    let (l, used) = layout_fitting("HELLO THERE", &block, 40, 4, LINE_GAP);
    assert!(
        used.height <= 4,
        "chose {} with height {}",
        used.name,
        used.height
    );
    assert!(l.rows(LINE_GAP) <= 4);
    assert_no_loss("HELLO THERE", &l);
}

#[test]
fn a_roomy_terminal_keeps_the_preferred_font() {
    let block = font::block();
    let (_, used) = layout_fitting("HELLO", &block, 200, 40, LINE_GAP);
    assert_eq!(used.name, "block");
}

#[test]
fn rendering_fills_exactly_the_area_it_was_given() {
    let f = font::block();
    let text = render(
        &Screen::Lyric {
            text: "HELLO",
            highlight: 0,
            reveal: 5,
            second: None,
        },
        &f,
        80,
        24,
        Theme::default(),
    );
    assert!(text.lines.len() <= 24, "overflowed the viewport");
    for line in &text.lines {
        let w: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        assert!(w <= 80, "line is {w} columns wide, viewport is 80");
    }
}

#[test]
fn the_highlight_splits_the_line_at_the_right_character() {
    let f = font::mini();
    let theme = Theme::default();
    let text = render(
        &Screen::Lyric {
            text: "abcdef",
            highlight: 3,
            reveal: 6,
            second: None,
        },
        &f,
        20,
        3,
        theme,
    );
    let row = text
        .lines
        .iter()
        .find(|l| l.spans.iter().any(|s| s.content.contains('a')))
        .expect("should have rendered the text");
    let sung: String = row
        .spans
        .iter()
        .filter(|s| s.style.fg == Some(theme.sung))
        .map(|s| s.content.to_string())
        .collect();
    assert_eq!(sung, "abc");
}

#[test]
fn a_blank_lyric_line_renders_nothing() {
    let f = font::block();
    let text = render(
        &Screen::Lyric {
            text: "   ",
            highlight: 0,
            reveal: 3,
            second: None,
        },
        &f,
        80,
        24,
        Theme::default(),
    );
    assert!(text.lines.is_empty());
}

#[test]
fn a_one_column_terminal_does_not_panic() {
    let f = font::block();
    let l = layout("HELLO WORLD", &f, 1);
    assert_no_loss("HELLO WORLD", &l);
    let _ = render(
        &Screen::Lyric {
            text: "HELLO WORLD",
            highlight: 2,
            reveal: 11,
            second: None,
        },
        &f,
        1,
        1,
        Theme::default(),
    );
}

/// One rendered row flattened back into the characters on screen.
fn row_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn a_partly_revealed_word_holds_the_place_of_the_whole_one() {
    // A word timed in syllables is drawn a piece at a time. Laying out only the
    // pieces sung so far would centre a fragment, and the word would crawl
    // sideways as it filled in. It must sit still.
    let f = font::mini();
    let theme = Theme::default();
    let whole = render(
        &Screen::Lyric {
            text: "believe",
            highlight: 0,
            reveal: 7,
            second: None,
        },
        &f,
        40,
        3,
        theme,
    );
    let part = render(
        &Screen::Lyric {
            text: "believe",
            highlight: 0,
            reveal: 2,
            second: None,
        },
        &f,
        40,
        3,
        theme,
    );

    assert_eq!(whole.lines.len(), part.lines.len(), "same rows");
    let mut inked_whole = 0usize;
    let mut inked_part = 0usize;
    for (w, p) in whole.lines.iter().zip(part.lines.iter()) {
        let (w, p) = (row_text(w), row_text(p));
        assert_eq!(w.len(), p.len(), "the row changed width as the word filled");
        for (col, (a, b)) in w.chars().zip(p.chars()).enumerate() {
            inked_whole += usize::from(!a.is_whitespace());
            inked_part += usize::from(!b.is_whitespace());
            if !b.is_whitespace() {
                assert_eq!(a, b, "column {col} moved");
            }
        }
    }
    assert!(inked_part > 0, "the sung syllable must be drawn");
    assert!(inked_part < inked_whole, "the rest must be held back");
}

/// The rows a rendered screen actually inked, as `(row index, colours used)`.
fn inked(text: &ratatui::text::Text<'_>) -> Vec<(usize, Vec<ratatui::style::Color>)> {
    text.lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.spans.iter().any(|s| s.content.trim() != ""))
        .map(|(i, l)| {
            let mut colours: Vec<_> = l
                .spans
                .iter()
                .filter(|s| s.content.trim() != "")
                .filter_map(|s| s.style.fg)
                .collect();
            colours.dedup();
            (i, colours)
        })
        .collect()
}

fn lyric<'a>(text: &'a str, second: Option<SecondVoice<'a>>) -> Screen<'a> {
    Screen::Lyric {
        text,
        highlight: 0,
        reveal: text.chars().count(),
        second,
    }
}

#[test]
fn the_second_voice_is_drawn_above_the_main_line() {
    let f = font::block();
    let out = render(
        &lyric(
            "LOVE",
            Some(SecondVoice {
                text: "OOH",
                background: true,
            }),
        ),
        &f,
        80,
        24,
        Theme::default(),
    );
    let rows = inked(&out);
    let first_dim = rows
        .iter()
        .position(|(_, c)| c.contains(&Theme::default().dim))
        .expect("the backing vocal is on screen");
    let first_lit = rows
        .iter()
        .position(|(_, c)| c.contains(&Theme::default().unsung))
        .expect("the line is on screen");
    assert!(
        first_dim < first_lit,
        "the second voice sits above the line"
    );
}

#[test]
fn a_background_voice_is_dimmed_and_a_size_smaller() {
    let theme = Theme::default();
    let out = render(
        &lyric(
            "LOVE",
            Some(SecondVoice {
                text: "OOH",
                background: true,
            }),
        ),
        &font::block(),
        80,
        24,
        theme,
    );
    let rows = inked(&out);
    let dim = rows.iter().filter(|(_, c)| c.contains(&theme.dim)).count();
    let lit = rows
        .iter()
        .filter(|(_, c)| c.contains(&theme.unsung))
        .count();
    assert_eq!(lit, 5, "the line is block art");
    assert_eq!(dim, 3, "the backing vocal is a size down, in compact");
}

#[test]
fn a_duet_partner_is_full_size_and_lit() {
    let theme = Theme::default();
    let out = render(
        &lyric(
            "LOVE",
            Some(SecondVoice {
                text: "NEED",
                background: false,
            }),
        ),
        &font::block(),
        80,
        24,
        theme,
    );
    let rows = inked(&out);
    assert!(
        rows.iter().all(|(_, c)| !c.contains(&theme.dim)),
        "the other half of a duet is a voice, not a shadow"
    );
    assert_eq!(
        rows.len(),
        10,
        "both voices are block art: five rows each, {rows:?}"
    );
}

#[test]
fn both_voices_step_down_together_before_either_is_dropped() {
    let theme = Theme::default();
    // Block (5) + gap (1) + compact (3) needs nine rows. In eight, both step
    // down rather than the second voice being dropped.
    let out = render(
        &lyric(
            "LOVE",
            Some(SecondVoice {
                text: "OOH",
                background: true,
            }),
        ),
        &font::block(),
        80,
        8,
        theme,
    );
    let rows = inked(&out);
    assert!(
        rows.iter().any(|(_, c)| c.contains(&theme.dim)),
        "the second voice is still there"
    );
    let lit = rows
        .iter()
        .filter(|(_, c)| c.contains(&theme.unsung))
        .count();
    assert!(
        lit < 5,
        "the line stepped down to make room, got {lit} rows"
    );
}

#[test]
fn the_main_line_keeps_its_solo_size_when_the_second_voice_is_dropped() {
    // The one thing that must never happen: shrinking the lyric to make room
    // for a voice that then does not fit anyway, leaving the reader with a
    // smaller line and nothing to show for it.
    let theme = Theme::default();
    let alone = render(&lyric("LOVE", None), &font::block(), 80, 5, theme);
    let paired = render(
        &lyric(
            "LOVE",
            Some(SecondVoice {
                // Long enough to wrap past five rows even in the smallest
                // font, so stepping down cannot rescue it and it has to go.
                text: "A BACKING VOCAL SO LONG THAT IT WRAPS PAST THE WHOLE \
                       BUDGET EVEN AS PLAIN TEXT AND THEREFORE CANNOT BE DRAWN \
                       ALONGSIDE ANYTHING AT ALL NO MATTER HOW FAR EITHER OF \
                       THE TWO VOICES STEPS DOWN IN SIZE FIRST",
                background: true,
            }),
        ),
        &font::block(),
        80,
        5,
        theme,
    );
    assert_eq!(
        inked(&alone).len(),
        inked(&paired).len(),
        "the line is drawn at the size it gets on its own"
    );
    assert!(
        inked(&paired).iter().all(|(_, c)| !c.contains(&theme.dim)),
        "and the voice that would not fit is simply not drawn"
    );
}

#[test]
fn a_blank_main_line_still_draws_its_second_voice() {
    // A line that is nothing but a backing vocal is rare but real; returning an
    // empty screen for it would blank the display mid-song.
    let theme = Theme::default();
    let out = render(
        &lyric(
            "",
            Some(SecondVoice {
                text: "OOH",
                background: true,
            }),
        ),
        &font::block(),
        80,
        24,
        theme,
    );
    assert!(!inked(&out).is_empty(), "something is on screen");
}

#[test]
fn the_font_helper_steps_down_monotonically() {
    // `next_after` is the `f` key's cycle and wraps; stepping down has to stop.
    assert_eq!(font::smaller_than("block").map(|f| f.name), Some("compact"));
    assert_eq!(font::smaller_than("compact").map(|f| f.name), Some("mini"));
    assert_eq!(font::smaller_than("mini").map(|f| f.name), None);
    assert_eq!(font::next_after("mini"), "block");
}
