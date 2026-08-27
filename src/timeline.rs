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

/// The word being sung: the whole word, plus how much of it has been reached.
///
/// The two differ when a source times a long word in syllables. The word is
/// laid out whole so it sits where it will finally sit, and revealed a syllable
/// at a time — showing only the sung syllables would put a bare fragment on the
/// screen, and laying out only those would make the word crawl as it grew.
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

    /// The second voice over line `index` at `pos`, if one is singing.
    ///
    /// Windowed rather than simply tied to the line, because a second voice
    /// rarely lines up with it: a background phrase usually comes in partway
    /// through and stops before the line does, and a duet partner's phrase ends
    /// while the line that took over runs on. Where two would be up at once the
    /// most recent wins, the same rule [`Timeline::locate`] uses for lines.
    ///
    /// It can never outlive its host: `locate` moves on at `line.end`.
    pub fn secondary(&self, index: usize, pos: f64) -> Option<&crate::lrc::Secondary> {
        self.lyrics
            .lines
            .get(index)?
            .secondary
            .iter()
            .rev()
            .find(|s| pos >= s.start && pos < s.display_end())
    }

    /// The word being sung within line `index`, and how far into it the singing
    /// has got.
    ///
    /// `None` before the first word starts. Inside a gap between words the
    /// previous word is kept rather than blanking the screen — the gaps are
    /// real (word-timed sources record them) and flashing through them would be
    /// worse than holding.
    pub fn active_word(&self, index: usize, pos: f64) -> Option<ActiveWord> {
        let line = self.lyrics.lines.get(index)?;
        let started = line.words.partition_point(|w| w.start <= pos);
        if started == 0 {
            return None;
        }
        let cur = started - 1;
        Some(ActiveWord {
            range: line.word_bounds(cur)?,
            sung_to: line.words[cur].range.end,
        })
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
