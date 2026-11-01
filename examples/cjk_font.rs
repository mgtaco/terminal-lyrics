//! Rebuilds `src/render/fonts/cjk.bin` from Fusion Pixel's 10px monospaced BDF
//! (https://github.com/TakWolf/fusion-pixel-font, the `ja` file).
//! `cargo run --example cjk_font -- fusion-pixel-10px-monospaced-ja.bdf`.
//!
//! Each record is 24 bytes, sorted by code point: the code point in three
//! bytes, the width in columns, then ten rows of `u16`, leftmost pixel in the
//! top bit. The rows run from the font's ascent down to its descent line.

use std::collections::BTreeMap;

const KEEP: &[std::ops::RangeInclusive<u32>] = &[
    0x3000..=0x30FF, // CJK punctuation, hiragana, katakana
    0x3100..=0x318F, // bopomofo, hangul compatibility jamo
    0x31F0..=0x31FF, // small katakana for Ainu
    0x3400..=0x4DBF, // Han extension A
    0x4E00..=0x9FFF, // Han
    0xAC00..=0xD7A3, // hangul syllables
    0xF900..=0xFAFF, // Han compatibility ideographs
    0xFF00..=0xFFEF, // fullwidth and halfwidth forms
];
const CELL: i32 = 10;
const ASCENT: i32 = 9;

fn main() {
    let path = std::env::args().nth(1).expect("path to the BDF file");
    let src = std::fs::read_to_string(&path).expect("readable BDF");
    let mut glyphs: BTreeMap<u32, (u8, [u16; CELL as usize])> = BTreeMap::new();

    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        let Some(code) = line.strip_prefix("ENCODING ") else {
            continue;
        };
        let code: u32 = code.trim().parse().expect("numeric ENCODING");
        let (mut advance, mut bbx) = (0, [0i32; 4]);
        for line in lines.by_ref() {
            if let Some(w) = line.strip_prefix("DWIDTH ") {
                advance = w.split(' ').next().unwrap().parse().unwrap();
            } else if let Some(b) = line.strip_prefix("BBX ") {
                for (slot, n) in bbx.iter_mut().zip(b.split(' ')) {
                    *slot = n.parse().unwrap();
                }
            } else if line == "BITMAP" {
                break;
            }
        }
        let [w, h, x_off, y_off] = bbx;
        let mut rows = [0u16; CELL as usize];
        let mut right = 0;
        for (i, hex) in lines.by_ref().take_while(|l| *l != "ENDCHAR").enumerate() {
            let bits = u64::from_str_radix(hex, 16).unwrap();
            let bit_len = hex.len() as i32 * 4;
            // Rows count down from the ascent; the BDF counts up from the baseline.
            let row = ASCENT - 1 - (y_off + h - 1 - i as i32);
            for x in 0..w {
                let col = x_off + x;
                if bits >> (bit_len - 1 - x) & 1 == 0 || !(0..CELL).contains(&col) {
                    continue;
                }
                if let Some(r) = usize::try_from(row).ok().and_then(|r| rows.get_mut(r)) {
                    *r |= 0x8000 >> col;
                    right = right.max(col + 1);
                }
            }
        }
        if KEEP.iter().any(|r| r.contains(&code)) && rows.iter().any(|r| *r != 0) {
            // The advance carries a blank column the fonts' tracking replaces.
            let width = (advance - 1).max(right).max(1) as u8;
            glyphs.insert(code, (width, rows));
        }
    }

    let mut out = Vec::with_capacity(glyphs.len() * 24);
    for (code, (width, rows)) in &glyphs {
        out.extend_from_slice(&code.to_le_bytes()[..3]);
        out.push(*width);
        for row in rows {
            out.extend_from_slice(&row.to_le_bytes());
        }
    }
    let dest = concat!(env!("CARGO_MANIFEST_DIR"), "/src/render/fonts/cjk.bin");
    std::fs::write(dest, &out).expect("writable font file");
    println!("{} glyphs, {} bytes -> {dest}", glyphs.len(), out.len());
}
