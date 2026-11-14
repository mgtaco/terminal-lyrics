//! Renders `docs/demo-dark.gif` and `docs/demo-light.gif`. Cells are drawn as
//! solid rectangles rather than through a font, so there are no hairlines
//! between blocks.
use std::fs::File;
use std::path::Path;

use anyhow::Result;
use terminal_lyrics::render::{self, Case, Screen, Theme, font};

/// The terminal the frames are laid out in. Only the inked area is kept.
const COLS: usize = 80;
const ROWS: usize = 12;
/// Pixels per cell. Two cells side by side make a square, as in a terminal.
const CELL_W: usize = 10;
const CELL_H: usize = 20;

/// Tokyo Night's foreground on GitHub's dark theme, Tokyo Night Day's on its
/// light one.
const VARIANTS: [(&str, [u8; 3]); 2] = [
    ("docs/demo-dark.gif", [0xc8, 0xd3, 0xf5]),
    ("docs/demo-light.gif", [0x37, 0x60, 0xbf]),
];

/// The word on screen, how many of its characters are inked, and how long it
/// stays up, in hundredths of a second.
const FRAMES: &[(&str, usize, u16)] = &[
    ("", 0, 66),
    ("one", 3, 49),
    ("word", 4, 58),
    ("at", 2, 41),
    ("a", 1, 33),
    ("time", 4, 91),
    ("syllable", 3, 33),
    ("syllable", 5, 24),
    ("syllable", 8, 49),
    ("by", 2, 49),
    ("syllable", 3, 33),
    ("syllable", 5, 24),
    ("syllable", 8, 91),
    ("straight", 8, 66),
    ("off", 3, 49),
    ("the", 3, 41),
    ("player", 6, 157),
    ("", 0, 49),
];

/// Inked half-cells: two rows per terminal row, top half first.
type Grid = Vec<Vec<bool>>;

/// The demo plays once in each case, lowercase first as the program starts.
const CASES: [Case; 2] = [Case::Lower, Case::Upper];

fn main() -> Result<()> {
    let frames: Vec<(&str, usize, u16, Case)> = CASES
        .iter()
        .flat_map(|&case| FRAMES.iter().map(move |&(t, r, d)| (t, r, d, case)))
        .collect();
    let grids = frames
        .iter()
        .map(|&(text, reveal, _, case)| ink(text, reveal, case))
        .collect::<Result<Vec<_>>>()?;

    // One crop for every frame, so the line never jumps — the capitals sit on
    // the same rows in both cases, with the lowercase descenders below them.
    let rows: Vec<usize> = (0..2 * ROWS)
        .filter(|&y| grids.iter().any(|g| g[y].contains(&true)))
        .collect();
    let cols: Vec<usize> = (0..COLS)
        .filter(|&x| grids.iter().any(|g| g.iter().any(|r| r[x])))
        .collect();
    let (Some(&y0), Some(&y1)) = (rows.first(), rows.last()) else {
        anyhow::bail!("no frame inked anything");
    };
    let (x0, x1) = (cols[0], cols[cols.len() - 1]);
    let half = CELL_H / 2;
    let pad = half;
    let width = (x1 + 1 - x0) * CELL_W + 2 * pad;
    let height = (y1 + 1 - y0) * half + 2 * pad;

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (path, rgb) in VARIANTS {
        let palette = [0, 0, 0, rgb[0], rgb[1], rgb[2]];
        let file = File::create(root.join(path))?;
        let mut enc = gif::Encoder::new(file, width as u16, height as u16, &palette)?;
        enc.set_repeat(gif::Repeat::Infinite)?;
        for (grid, &(_, _, delay, _)) in grids.iter().zip(&frames) {
            let mut px = vec![0u8; width * height];
            for (y, cells) in grid[y0..=y1].iter().enumerate() {
                for x in (x0..=x1).filter(|&x| cells[x]) {
                    let left = pad + (x - x0) * CELL_W;
                    for row in pad + y * half..pad + (y + 1) * half {
                        px[row * width + left..row * width + left + CELL_W].fill(1);
                    }
                }
            }
            let mut frame =
                gif::Frame::from_indexed_pixels(width as u16, height as u16, px, Some(0));
            frame.delay = delay;
            // Clear before the next frame: on a transparent background the
            // default keeps the previous word showing through.
            frame.dispose = gif::DisposalMethod::Background;
            enc.write_frame(&frame)?;
        }
        println!("{path}: {width}x{height}, {} frames", frames.len());
    }
    Ok(())
}

/// Which cells the renderer inks for this frame.
fn ink(text: &str, reveal: usize, case: Case) -> Result<Grid> {
    let cased = case.apply(text);
    let screen = Screen::Lyric {
        text: &cased,
        highlight: 0,
        reveal,
        second: None,
    };
    let rendered = render::render(
        &screen,
        &font::block(),
        COLS as u16,
        ROWS as u16,
        Theme::default(),
    );
    let mut grid = vec![vec![false; COLS]; 2 * ROWS];
    for (y, line) in rendered.lines.iter().enumerate().take(ROWS) {
        let chars = line.spans.iter().flat_map(|s| s.content.chars());
        for (x, c) in chars.enumerate().take(COLS) {
            let (top, bottom) = match c {
                ' ' => (false, false),
                '▀' => (true, false),
                '▄' => (false, true),
                '█' => (true, true),
                _ => anyhow::bail!("{c:?} in {text:?}: only blocks can be drawn as cells"),
            };
            grid[2 * y][x] = top;
            grid[2 * y + 1][x] = bottom;
        }
    }
    Ok(grid)
}
