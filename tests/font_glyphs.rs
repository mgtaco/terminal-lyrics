//! No Latin, Cyrillic or Greek letter is drawn blank, accents land where they should, and no two
//! characters look the same.

use terminal_lyrics::render::font::{self, Font, Glyph};
use terminal_lyrics::render::layout::layout;

/// Everything drawn by hand in `src/render/fonts/`.
const DRAWN: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789\
    ÆæŒœØøŁłĐđÐðĦħÞþßẞŊŋəıȷĲĳĿŀĸŉªº\
    '\"‘’“”`,.…!¡?¿-–—_()[]:;/\\&#*+=<>@$%~^«»♪♫·\
    БГДЖЗИЛПУФЧШЩЦЪЬЫЄЭЮЯҐЉЊЋЂЏбвгджзиклмнптфцчшщъьыєэюяґљњђџ\
    ΔΘΛΞΣΨΩαβγδεζηθλμξπρσςτυχψω";

/// Pairs that are meant to look alike.
const TWINS: &[(char, char)] = &[
    ('\'', '’'),
    ('"', '”'),
    ('Đ', 'Ð'),
    // Compact is too small for curly quotes to curl.
    ('\'', '‘'),
    ('’', '‘'),
    ('"', '“'),
    ('”', '“'),
    ('З', '3'),
    ('к', 'ĸ'),
    // In compact a capital is no taller than an ascender.
    ('Ь', 'b'),
];

fn fonts() -> [Font; 2] {
    [font::block(), font::compact()]
}

fn art(f: &Font, c: char) -> terminal_lyrics::render::font::Bitmap {
    match f.draw(c, &[]) {
        Glyph::Art(b) => b,
        Glyph::Plain(s) => panic!("{} has no art for {c:?}, drew {s:?}", f.name),
    }
}

/// The glyph's pixels, ignoring where the canvas starts.
fn picture(f: &Font, c: char) -> Vec<Vec<bool>> {
    let b = art(f, c);
    let Some((top, bottom)) = b.ink_rows() else {
        return Vec::new();
    };
    (top..=bottom)
        .map(|y| (0..b.width).map(|x| b.ink(x, y)).collect())
        .collect()
}

#[test]
fn every_hand_drawn_character_has_ink() {
    for f in fonts() {
        for c in DRAWN.chars() {
            assert!(!art(&f, c).is_blank(), "{} draws {c:?} blank", f.name);
        }
    }
}

#[test]
fn no_two_characters_look_the_same() {
    for f in fonts() {
        let chars: Vec<char> = DRAWN.chars().collect();
        let mut alike = Vec::new();
        for (i, a) in chars.iter().enumerate() {
            for b in &chars[i + 1..] {
                if TWINS.contains(&(*a, *b)) || TWINS.contains(&(*b, *a)) {
                    continue;
                }
                // Where a shape sits counts too: an apostrophe and a comma
                // can be the same pixels at different heights.
                if picture(&f, *a) == picture(&f, *b)
                    && art(&f, *a).ink_rows() == art(&f, *b).ink_rows()
                {
                    alike.push(format!("{a}{b}"));
                }
            }
        }
        assert!(alike.is_empty(), "{} draws these alike: {alike:?}", f.name);
    }
}

/// Latin-1 Supplement and Latin Extended-A: Western, Central and Northern
/// European, Turkish, Baltic. Plus Romanian's comma-below letters.
fn latin_letters() -> impl Iterator<Item = char> {
    ('\u{C0}'..='\u{17F}')
        .chain("ȘșȚț".chars())
        .filter(|c| c.is_alphabetic())
}

const VIETNAMESE: &str = "ẠạẢảẤấẦầẨẩẪẫẬậẮắẰằẲẳẴẵẶặẸẹẺẻẼẽẾếỀềỂểỄễỆệỈỉỊịỌọỎỏỐốỒồỔổỖỗỘộ\
    ỚớỜờỞởỠỡỢợỤụỦủỨứỪừỬửỮữỰựỲỳỴỵỶỷỸỹƠơƯư";

#[test]
fn every_european_letter_is_drawn_as_art() {
    for f in fonts() {
        let missing: String = latin_letters()
            .chain(VIETNAMESE.chars())
            .filter(|c| !matches!(f.draw(*c, &[]), Glyph::Art(b) if !b.is_blank()))
            .collect();
        assert!(missing.is_empty(), "{} cannot draw {missing:?}", f.name);
    }
}

/// The basic Cyrillic block: Russian, Ukrainian, Belarusian, Bulgarian,
/// Serbian and Macedonian.
#[test]
fn every_cyrillic_letter_is_drawn_as_art() {
    for f in fonts() {
        let missing: String = ('\u{400}'..='\u{45F}')
            .chain("Ґґ".chars())
            .filter(|c| !matches!(f.draw(*c, &[]), Glyph::Art(b) if !b.is_blank()))
            .collect();
        assert!(missing.is_empty(), "{} cannot draw {missing:?}", f.name);
    }
}

/// Modern Greek, with its accented letters.
#[test]
fn every_greek_letter_is_drawn_as_art() {
    for f in fonts() {
        let missing: String = ('\u{386}'..='\u{3CE}')
            .filter(|c| c.is_alphabetic())
            .filter(|c| !matches!(f.draw(*c, &[]), Glyph::Art(b) if !b.is_blank()))
            .collect();
        assert!(missing.is_empty(), "{} cannot draw {missing:?}", f.name);
    }
}

#[test]
fn an_accent_changes_the_letter_and_rises_above_a_capital() {
    for f in fonts() {
        assert_ne!(picture(&f, 'é'), picture(&f, 'e'), "{}", f.name);
        assert_ne!(picture(&f, 'é'), picture(&f, 'è'), "{}", f.name);
        let (plain, _) = art(&f, 'E').ink_rows().unwrap();
        let (accented, _) = art(&f, 'É').ink_rows().unwrap();
        assert_eq!(plain, 0, "a capital starts on the cap line");
        assert!(
            accented < 0,
            "{}: É's acute should stand above the E",
            f.name
        );
        // On a lowercase letter the accent fits under the cap line.
        let (low, _) = art(&f, 'é').ink_rows().unwrap();
        assert!(low >= -1, "{}: é's acute rose to {low}", f.name);
    }
}

#[test]
fn an_accented_i_loses_its_dot() {
    // A dot and an accent do not stack: í is a dotless ı under an acute, and
    // Ukrainian ї is a dotless ı under a diaeresis.
    for (i, accented, mark) in [('i', 'í', '\u{301}'), ('і', 'ї', '\u{308}')] {
        for f in fonts() {
            let composed = match f.draw('ı', &[mark]) {
                Glyph::Art(b) => b,
                Glyph::Plain(_) => unreachable!(),
            };
            assert_eq!(art(&f, accented), composed, "{}: {accented}", f.name);
            assert_ne!(picture(&f, accented), picture(&f, i), "{}", f.name);
        }
    }
}

#[test]
fn a_plain_capital_line_keeps_its_height_and_overhangs_only_when_needed() {
    let f = font::block();
    let plain = layout("HELLO", &f, 200);
    assert_eq!(plain.lines[0].height, 5);
    assert_eq!((plain.lines[0].above, plain.lines[0].below), (0, 0));

    let both = layout("Ángel", &f, 200);
    let line = &both.lines[0];
    assert!(line.above > 0, "Á needs room above");
    assert!(line.below > 0, "g needs room below");
    assert_eq!(line.height, 5 + line.above + line.below);

    let low = layout("señor", &f, 200);
    assert_eq!(
        (low.lines[0].above, low.lines[0].below),
        (0, 0),
        "a lowercase accent fits inside the capitals' rows"
    );
}

#[test]
fn decomposed_text_draws_the_same_as_precomposed() {
    for f in fonts() {
        let nfc = layout("café", &f, 200);
        let nfd = layout("cafe\u{301}", &f, 200);
        let rows = |l: &terminal_lyrics::render::layout::Layout| {
            l.lines[0]
                .cells
                .iter()
                .map(|c| (c.rows.clone(), c.src))
                .collect::<Vec<_>>()
        };
        assert_eq!(rows(&nfc), rows(&nfd), "{}", f.name);
        assert_eq!(f.draw('ё', &[]), f.draw('е', &['\u{308}']), "{}", f.name);
        assert_eq!(f.draw('Й', &[]), f.draw('И', &['\u{306}']), "{}", f.name);
    }
}

#[test]
fn a_line_in_another_script_falls_back_to_plain_text() {
    let l = layout("سلام", &font::block(), 200);
    assert_eq!(l.lines[0].height, 1);
    let text: String = l.lines[0].cells.iter().map(|c| c.rows[0].clone()).collect();
    assert_eq!(text, "سلام");
}

#[test]
fn chinese_japanese_and_korean_are_drawn_as_art() {
    for f in fonts() {
        for c in "君の名はカタカナ中文漢字한국어、。「」！ｶ".chars() {
            assert!(
                matches!(f.draw(c, &[]), Glyph::Art(b) if !b.is_blank()),
                "{}: {c} is not art",
                f.name
            );
        }
    }
    // Exactly the capitals' height, so a line of it stands where Latin would.
    let l = layout("君の名は", &font::block(), 200);
    assert_eq!(l.lines[0].height, 5);
}

#[test]
fn decomposed_kana_keeps_its_dakuten() {
    for f in fonts() {
        assert_eq!(f.draw('が', &[]), f.draw('か', &['\u{3099}']), "{}", f.name);
        assert_ne!(f.draw('が', &[]), f.draw('か', &[]), "{}", f.name);
    }
}

#[test]
fn a_stray_symbol_is_shown_in_place_rather_than_dropped() {
    let l = layout("hi 🎵", &font::block(), 200);
    let line = &l.lines[0];
    assert_eq!(line.height, 5, "still block art");
    let emoji = line.cells.last().unwrap();
    assert_eq!(emoji.src, 3);
    assert!(emoji.rows.iter().any(|r| r.contains('🎵')));
}

#[test]
fn every_row_of_a_cell_is_its_full_width() {
    for f in fonts() {
        let l = layout("Ŀa Ćmy ȘI ọ́ 🎵 x", &f, 200);
        for cell in l.lines.iter().flat_map(|l| &l.cells) {
            for row in &cell.rows {
                assert_eq!(
                    unicode_width::UnicodeWidthStr::width(row.as_str()),
                    cell.width,
                    "{}: {row:?}",
                    f.name
                );
            }
        }
    }
}
