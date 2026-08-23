//! Turning a playback position into "what should be on screen right now".
//!
//! Two separate questions: which line is active (binary search), and how much of
//! it has been sung (the sweep). The sweep is interpolated when the source has
//! no word tags — that interpolation is display-only and is never written back
//! out. Guessed timings that get saved to disk stop looking like guesses.

use std::ops::Range;

use crate::lrc::Lyrics;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Position {
    /// Playback is before the first timestamp; the intro is still running.
    Intro { until: f64 },
    /// `index` into [`Lyrics::lines`].
    Line { index: usize },
    /// Past the end of the last line.
    Outro,
}

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

    /// The word being sung within line `index`, as a char range into
    /// [`crate::lrc::Line::text`].
    ///
    /// `None` before the first word starts. Inside a gap between words the
    /// previous word is kept rather than blanking the screen — the gaps are
    /// real (word-timed sources record them) and flashing through them would be
    /// worse than holding.
    pub fn active_word(&self, index: usize, pos: f64) -> Option<Range<usize>> {
        let line = self.lyrics.lines.get(index)?;
        let started = line.words.partition_point(|w| w.start <= pos);
        if started == 0 {
            return None;
        }
        Some(line.words[started - 1].range.clone())
    }

    /// How far the highlight has travelled across line `index`, in characters.
    ///
    /// With real word tags the sweep follows them. Without, it moves linearly
    /// across the line between its two real timestamps, weighted by character
    /// count — an estimate, and labelled as one by [`Lyrics::has_word_timings`].
    pub fn highlight_chars(&self, index: usize, pos: f64) -> usize {
        let Some(line) = self.lyrics.lines.get(index) else {
            return 0;
        };
        let total = line.char_len();
        if total == 0 {
            return 0;
        }

        if !line.words.is_empty() {
            let words = &line.words;
            // Last word that has already started.
            let started = words.partition_point(|w| w.start <= pos);
            if started == 0 {
                return 0;
            }
            let w = &words[started - 1];
            let span = w.end - w.start;
            let frac = if span > f64::EPSILON {
                ((pos - w.start) / span).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let width = w.range.end.saturating_sub(w.range.start) as f64;
            return (w.range.start + (width * frac).round() as usize).min(total);
        }

        let span = line.end - line.start;
        if span <= f64::EPSILON {
            return total;
        }
        let frac = ((pos - line.start) / span).clamp(0.0, 1.0);
        ((total as f64) * frac).round() as usize
    }
}
