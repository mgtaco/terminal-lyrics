//! Spectrum bars, drawn in eighth blocks, or half blocks on the Linux console,
//! whose fonts have no other eighths.

use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};

/// Columns per bar, and between bars. Two wide reads as a bar rather than a
/// line, and the gap keeps neighbours from merging into a skyline.
pub const BAR_WIDTH: usize = 2;
pub const BAR_GAP: usize = 1;

const EIGHTHS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Always even: cava's stereo output mirrors one channel against the other.
pub fn bar_count(width: usize) -> usize {
    let n = (width + BAR_GAP) / (BAR_WIDTH + BAR_GAP);
    n - n % 2
}

/// Bars that do not fit (a frame from before a resize) are dropped from both
/// ends, so the middle, where stereo puts the bass, stays put.
pub fn render(
    levels: &[u16],
    max: u16,
    rows: usize,
    width: usize,
    style: Style,
    console: bool,
) -> Text<'static> {
    let steps = rows * 8;
    let max = usize::from(max.max(1));
    // Rounded, so a whisper of noise does not leave a sliver under every bar.
    let scale = |level: u16| (usize::from(level) * steps + max / 2) / max;
    let fits = bar_count(width);
    let skip = levels.len().saturating_sub(fits) / 2;
    let shown = &levels[skip..levels.len().min(skip + fits)];
    let used = (shown.len() * (BAR_WIDTH + BAR_GAP)).saturating_sub(BAR_GAP);
    let pad = " ".repeat(width.saturating_sub(used) / 2);

    (0..rows)
        .map(|row| {
            // How many eighths sit below this row's cell.
            let floor = (rows - 1 - row) * 8;
            let mut text = pad.clone();
            for (i, &level) in shown.iter().enumerate() {
                if i > 0 {
                    text.extend(std::iter::repeat_n(' ', BAR_GAP));
                }
                let mut fill = scale(level).saturating_sub(floor).min(8);
                if console {
                    fill = (fill + 2) / 4 * 4;
                }
                text.extend(std::iter::repeat_n(EIGHTHS[fill], BAR_WIDTH));
            }
            Line::from(Span::styled(text.trim_end().to_string(), style))
        })
        .collect::<Vec<_>>()
        .into()
}
