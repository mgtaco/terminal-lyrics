//! The sweep is interpolated when the source has no word tags; that guess is
//! display-only and never written to disk.

use std::ops::Range;

use crate::lrc::{Lyrics, Word};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Position {
    /// Playback is before the first timestamp; the intro is still running.
    Intro { until: f64 },
    /// `index` into [`Lyrics::lines`].
    Line { index: usize },
    /// Past the end of the last line.
    Outro,
}

/// Laid out whole and revealed a syllable at a time: laying out only the sung
/// syllables would make the word crawl as it grew.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveWord {
    /// Char range of the whole word in [`crate::lrc::Line::text`].
    pub range: Range<usize>,
    /// End of the syllable reached so far, as a char index into the line.
    pub sung_to: usize,
}

impl ActiveWord {
    /// How many of the word's characters have been reached.
    pub fn revealed(&self) -> usize {
        self.sung_to.saturating_sub(self.range.start)
    }
}

/// The shortest break in the middle of a song that is counted down; anything
/// shorter keeps the last line on screen.
pub const MIN_BREAK: f64 = 5.0;

#[derive(Debug, Clone)]
pub struct Timeline {
    lyrics: Lyrics,
}

impl Timeline {
    pub fn new(lyrics: Lyrics) -> Self {
        Self { lyrics }
    }

    pub fn lyrics(&self) -> &Lyrics {
        &self.lyrics
    }

    pub fn len(&self) -> usize {
        self.lyrics.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lyrics.lines.is_empty()
    }

    pub fn line(&self, index: usize) -> Option<&crate::lrc::Line> {
        self.lyrics.lines.get(index)
    }

    /// Which line covers `pos`. O(log n) — the display asks this ~30 times a
    /// second, and a long song is a few hundred lines.
    pub fn locate(&self, pos: f64) -> Position {
        let lines = &self.lyrics.lines;
        let Some(first) = lines.first() else {
            return Position::Outro;
        };
        if pos < first.start {
            return Position::Intro { until: first.start };
        }
        // Index of the last line whose start is <= pos.
        let idx = lines.partition_point(|l| l.start <= pos) - 1;
        let line = &lines[idx];
        if idx + 1 == lines.len() && pos >= line.end {
            return Position::Outro;
        }
        Position::Line { index: idx }
    }

    /// The wordless stretch `pos` is in, as when it began and when the next
    /// line starts: the intro, a blank line, or a word-timed line that has
    /// finished, when the break is at least [`MIN_BREAK`].
    pub fn wait(&self, pos: f64) -> Option<(f64, f64)> {
        let lines = &self.lyrics.lines;
        let first = lines.first()?;
        if pos < first.start {
            return Some((0.0, first.start));
        }
        let idx = lines.partition_point(|l| l.start <= pos) - 1;
        let (line, next) = (&lines[idx], lines.get(idx + 1)?);
        let start = match line.is_blank() && line.secondary.is_empty() {
            true => line.start,
            false => line.words.last().map(|w| w.end).filter(|e| e.is_finite())?,
        };
        let quiet = pos >= start && self.secondary(idx, pos).is_none();
        (quiet && next.start - start >= MIN_BREAK).then_some((start, next.start))
    }

    /// Windowed rather than tied to the line, since a second voice rarely lines
    /// up with it; where two overlap the most recent wins, as in
    /// [`Timeline::locate`].
    pub fn secondary(&self, index: usize, pos: f64) -> Option<&crate::lrc::Secondary> {
        self.lyrics
            .lines
            .get(index)?
            .secondary
            .iter()
            .rev()
            .find(|s| pos >= s.start && pos < s.display_end())
    }

    /// `None` before the first word. In a gap between words the previous word
    /// is held: flashing through real gaps would be worse than holding.
    pub fn active_word(&self, index: usize, pos: f64) -> Option<ActiveWord> {
        let line = self.lyrics.lines.get(index)?;
        active_word_in(&line.text, &line.words, pos)
    }

    /// [`Timeline::active_word`] for the second voice; `None` for a voice
    /// without word timings, which is drawn whole.
    pub fn second_active_word(&self, index: usize, pos: f64) -> Option<ActiveWord> {
        let s = self.secondary(index, pos)?;
        active_word_in(&s.text, &s.words, pos)
    }

    /// Only real word timings move it: the partner's window starts where it was
    /// cut off, so there is nothing honest to interpolate. Always 0 for a
    /// backing vocal.
    pub fn second_highlight_chars(&self, index: usize, pos: f64) -> usize {
        let Some(s) = self.secondary(index, pos).filter(|s| !s.background) else {
            return 0;
        };
        sweep_words(&s.words, s.text.chars().count(), pos).unwrap_or(0)
    }

    /// Follows word tags when there are any; otherwise an estimate, linear
    /// across the line by character count.
    pub fn highlight_chars(&self, index: usize, pos: f64) -> usize {
        let Some(line) = self.lyrics.lines.get(index) else {
            return 0;
        };
        let total = line.char_len();
        if total == 0 {
            return 0;
        }

        if let Some(chars) = sweep_words(&line.words, total, pos) {
            return chars;
        }

        let span = line.end - line.start;
        if span <= f64::EPSILON {
            return total;
        }
        let frac = ((pos - line.start) / span).clamp(0.0, 1.0);
        ((total as f64) * frac).round() as usize
    }
}

/// The word being sung in a voice with these word timings. See
/// [`Timeline::active_word`].
fn active_word_in(text: &str, words: &[Word], pos: f64) -> Option<ActiveWord> {
    let started = words.partition_point(|w| w.start <= pos);
    if started == 0 {
        return None;
    }
    let cur = started - 1;
    Some(ActiveWord {
        range: crate::lrc::word_bounds(text, words, cur)?,
        sung_to: words[cur].range.end,
    })
}

/// How far real word timings have swept across a voice `total` characters long.
/// `None` when there are no word timings to follow.
fn sweep_words(words: &[Word], total: usize, pos: f64) -> Option<usize> {
    if words.is_empty() {
        return None;
    }
    // Last word that has already started.
    let started = words.partition_point(|w| w.start <= pos);
    if started == 0 {
        return Some(0);
    }
    let w = &words[started - 1];
    let span = w.end - w.start;
    let frac = if span > f64::EPSILON {
        ((pos - w.start) / span).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let width = w.range.end.saturating_sub(w.range.start) as f64;
    Some((w.range.start + (width * frac).round() as usize).min(total))
}
