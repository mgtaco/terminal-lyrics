//! The one LRC parser, since two that differ make lines vanish on playback.
//! Timed lines with no text are kept: they end the previous line for a break.

use std::ops::Range;

/// One word, or one syllable of a word when the source times long words in
/// pieces; [`Line::continues_word`] tells them apart.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub start: f64,
    /// From a trailing `<mm:ss.xx>` tag, else the next word's start. A real gap
    /// between words needs it, or the highlight smears across the pause.
    pub end: f64,
    /// Char range of this word inside [`Line::text`], for highlighting.
    pub range: Range<usize>,
}

/// A second voice singing over a line: a background vocal ("(ooh ooh)"), or the
/// other half of a duet whose phrase is still running when this line comes in.
#[derive(Debug, Clone, PartialEq)]
pub struct Secondary {
    /// When it comes in. Not the host line's start: a background phrase usually
    /// begins partway through the line it sits over.
    pub start: f64,
    /// When the source stops it.
    pub end: f64,
    pub text: String,
    /// Measured in its own `text`, so it can go word by word alongside the
    /// line. A background vocal steps through its words too, but is never
    /// swept.
    pub words: Vec<Word>,
    /// Background vocals are dimmed and drawn a size smaller. The other half of
    /// a duet is neither: it is a co-equal voice.
    pub background: bool,
}

/// "(ooh)" can be a fifth of a second long, which reads as flicker.
/// Display-only: never written to disk.
pub const MIN_SECONDARY_HOLD: f64 = 1.0;

impl Secondary {
    /// When it leaves the screen: its own end, or [`MIN_SECONDARY_HOLD`] after
    /// it arrived, whichever is later.
    pub fn display_end(&self) -> f64 {
        self.end.max(self.start + MIN_SECONDARY_HOLD)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub start: f64,
    /// End of the line: the next line's start, or an estimate for the last one.
    pub end: f64,
    pub text: String,
    /// Real per-word timings. Empty when the source is line-level only —
    /// never synthesised here, so callers can tell the difference.
    pub words: Vec<Word>,
    /// Voices singing over this line, in start order. Usually empty; at most
    /// one is on screen at a time — see [`crate::timeline::Timeline::secondary`].
    pub secondary: Vec<Secondary>,
}

impl Line {
    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    /// Only whitespace separates two real words, so a span butted against the
    /// one before it is a syllable of the same word (`be`, `lieve`).
    pub fn continues_word(&self, i: usize) -> bool {
        continues_word(&self.text, &self.words, i)
    }

    /// The char range of the whole word that word `i` belongs to, from the start
    /// of its first syllable to the end of its last.
    pub fn word_bounds(&self, i: usize) -> Option<Range<usize>> {
        word_bounds(&self.text, &self.words, i)
    }
}

/// [`Line::continues_word`], for any voice: a line or a duet partner.
pub fn continues_word(text: &str, words: &[Word], i: usize) -> bool {
    let Some(prev) = i.checked_sub(1).and_then(|p| words.get(p)) else {
        return false;
    };
    let Some(cur) = words.get(i) else {
        return false;
    };
    let gap = cur.range.start.saturating_sub(prev.range.end);
    text.chars()
        .skip(prev.range.end)
        .take(gap)
        .all(|c| !c.is_whitespace())
}

/// [`Line::word_bounds`], for any voice: a line or a duet partner.
pub fn word_bounds(text: &str, words: &[Word], i: usize) -> Option<Range<usize>> {
    words.get(i)?;
    let mut first = i;
    while continues_word(text, words, first) {
        first -= 1;
    }
    let mut last = i;
    while continues_word(text, words, last + 1) {
        last += 1;
    }
    Some(words[first].range.start..words[last].range.end)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub author: Option<String>,
    /// `[length:mm:ss]`, in seconds.
    pub length: Option<f64>,
    /// `[offset:ms]` as written; already applied to the timestamps.
    pub offset_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    pub meta: Meta,
}

impl Lyrics {
    /// True when at least one line carries real per-word timings.
    pub fn has_word_timings(&self) -> bool {
        self.lines.iter().any(|l| !l.words.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.is_blank())
    }

    /// Plain text, one line per lyric line — used for unsynced fallbacks.
    pub fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Parse `[mm:ss(.frac)?]` starting at `chars[i]`, returning the time and the
/// index just past the closing bracket. Minutes may exceed 99.
fn parse_timestamp(s: &[char], i: usize) -> Option<(f64, usize)> {
    if s.get(i) != Some(&'[') {
        return None;
    }
    let close = s[i..].iter().position(|&c| c == ']')? + i;
    let body: String = s[i + 1..close].iter().collect();
    let secs = parse_clock(&body)?;
    Some((secs, close + 1))
}

/// `mm:ss`, `mm:ss.xx`, `hh:mm:ss.xx` → seconds. Rejecting anything else is
/// what keeps `[ti:Song]` from being read as a time.
pub fn parse_clock(body: &str) -> Option<f64> {
    let mut parts = body.split(':');
    let a = parts.next()?;
    let b = parts.next()?;
    let c = parts.next();
    if parts.next().is_some() {
        return None;
    }

    let (h, m, rest) = match c {
        Some(c) => (a.parse::<u64>().ok()?, b.parse::<u64>().ok()?, c),
        None => (0, a.parse::<u64>().ok()?, b),
    };

    // Seconds may carry a fraction; both `.` and `,` appear in the wild.
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let normalised = rest.replace(',', ".");
    if !normalised.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let s: f64 = normalised.parse().ok()?;
    if !s.is_finite() || s < 0.0 {
        return None;
    }
    Some(h as f64 * 3600.0 + m as f64 * 60.0 + s)
}

/// `<mm:ss.xx>` word tag at `chars[i]`.
fn parse_word_tag(s: &[char], i: usize) -> Option<(f64, usize)> {
    if s.get(i) != Some(&'<') {
        return None;
    }
    let close = s[i..].iter().position(|&c| c == '>')? + i;
    let body: String = s[i + 1..close].iter().collect();
    let secs = parse_clock(&body)?;
    Some((secs, close + 1))
}

/// An ID tag such as `[ar:Radiohead]`, returned as `(key, value)`.
fn parse_id_tag(line: &str) -> Option<(String, String)> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    let (key, value) = inner.split_once(':')?;
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some((key, value.trim().to_string()))
}

/// A line body split into text plus any word timings it carried.
struct Body {
    text: String,
    words: Vec<Word>,
}

fn parse_body(rest: &[char]) -> Body {
    let mut text = String::new();
    let mut words: Vec<Word> = Vec::new();
    let mut pending: Option<(f64, usize)> = None; // (start, char index in `text`)
    let mut chars = 0usize;
    let mut i = 0usize;

    while i < rest.len() {
        if let Some((t, next)) = parse_word_tag(rest, i) {
            if let Some((start, from)) = pending.take() {
                push_word(&mut words, &text, start, from, chars);
            }
            pending = Some((t, chars));
            i = next;
            continue;
        }
        text.push(rest[i]);
        chars += 1;
        i += 1;
    }

    if let Some((start, from)) = pending.take() {
        push_word(&mut words, &text, start, from, chars);
    }

    // Trailing whitespace would otherwise widen the last word's highlight.
    let trimmed = text.trim_end();
    if trimmed.len() != text.len() {
        let new_len = trimmed.chars().count();
        text.truncate(trimmed.len());
        for w in &mut words {
            w.range.end = w.range.end.min(new_len);
            w.range.start = w.range.start.min(w.range.end);
        }
    }

    Body { text, words }
}

/// A tag with no visible text behind it is an end tag: it closes the previous
/// word rather than opening a new one.
fn push_word(words: &mut Vec<Word>, text: &str, start: f64, from: usize, to: usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut end = to;
    while end > from && chars.get(end - 1).is_some_and(|c| c.is_whitespace()) {
        end -= 1;
    }
    if end <= from {
        if let Some(prev) = words.last_mut()
            && start >= prev.start
        {
            prev.end = start;
        }
        return;
    }
    words.push(Word {
        start,
        end: f64::INFINITY, // resolved in `finalise`
        range: from..end,
    });
}

/// `[bg:start]` names the line a second voice sings over, `[end:]` when the
/// singing stops. They go ahead of the body, so word ranges still count from
/// the text.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Markers {
    background: bool,
    /// Start timestamp of the line this one sings over. A parent pointer, not a
    /// duration.
    parent: Option<f64>,
    end: Option<f64>,
}

/// Stops at the first bracket group that is not a marker, so `[00:10.00][verse
/// 1]words` still reads as `[verse 1]words`.
fn parse_markers(s: &[char], mut i: usize) -> (Markers, usize) {
    let mut out = Markers::default();
    while s.get(i) == Some(&'[') {
        let Some(close) = s[i..].iter().position(|&c| c == ']').map(|p| p + i) else {
            break;
        };
        let inner: String = s[i + 1..close].iter().collect();
        let (key, value) = match inner.split_once(':') {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), Some(v.trim())),
            None => (inner.trim().to_ascii_lowercase(), None),
        };
        match key.as_str() {
            "bg" => {
                out.background = true;
                out.parent = value.and_then(parse_clock);
            }
            "end" => match value.and_then(parse_clock) {
                Some(t) => out.end = Some(t),
                // `[end]` with nothing readable behind it is not a marker; let
                // it be text rather than silently swallowing part of a line.
                None => break,
            },
            _ => break,
        }
        i = close + 1;
    }
    (out, i)
}

/// `real_end` is when the singing stops, not [`Line::end`], when the line
/// leaves the screen; it and the background marker mean nothing after
/// [`finalise`].
struct Draft {
    start: f64,
    real_end: Option<f64>,
    background: bool,
    parent: Option<f64>,
    text: String,
    words: Vec<Word>,
}

impl Draft {
    /// When the singing stops, as far as the source is willing to say: the
    /// `[end:]` marker, or failing that a trailing A2 tag closing the last word.
    fn sung_end(&self) -> Option<f64> {
        self.real_end
            .or_else(|| self.words.last().map(|w| w.end).filter(|e| e.is_finite()))
    }
}

/// Anything less is timing slop: across 95 overlapping pairs in the AMLL
/// database the median overlap is 0.25 s. `ttm:agent` cannot tell the cases
/// apart, since 42 of the 95 share one.
const MIN_DUET_OVERLAP: f64 = 1.0;
const MIN_DUET_OVERLAP_FRACTION: f64 = 0.5;

/// Two timestamps this close are the same timestamp. A millisecond is the
/// finest thing any of these formats can write.
const SAME_TIME: f64 = 0.001;

/// Parse an LRC document. Never fails: malformed lines are skipped, because a
/// single bad line in a downloaded file should not cost the user their lyrics.
pub fn parse(input: &str) -> Lyrics {
    let mut meta = Meta::default();
    let mut lines: Vec<Draft> = Vec::new();

    for raw in input.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let chars: Vec<char> = trimmed.chars().collect();

        // Collect every leading `[..]` that parses as a clock.
        let mut stamps: Vec<f64> = Vec::new();
        let mut i = 0usize;
        while let Some((t, next)) = parse_timestamp(&chars, i) {
            stamps.push(t);
            i = next;
        }

        if stamps.is_empty() {
            if let Some((key, value)) = parse_id_tag(trimmed) {
                match key.as_str() {
                    "ti" => meta.title = Some(value),
                    "ar" => meta.artist = Some(value),
                    "al" => meta.album = Some(value),
                    "by" => meta.author = Some(value),
                    "length" => meta.length = parse_clock(&value),
                    "offset" => {
                        if let Ok(ms) = value.trim_start_matches('+').parse::<i64>() {
                            meta.offset_ms = ms;
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }

        let (markers, i) = parse_markers(&chars, i);
        let body = parse_body(&chars[i..]);
        for start in stamps {
            lines.push(Draft {
                start,
                real_end: markers.end,
                background: markers.background,
                parent: markers.parent,
                text: body.text.clone(),
                words: body.words.clone(),
            });
        }
    }

    finalise(lines, meta)
}

/// The offset is applied once at the end, which is why every `end` moves with
/// it rather than only the starts.
fn finalise(drafts: Vec<Draft>, meta: Meta) -> Lyrics {
    // Stable, so two lines that begin together keep their written order and the
    // later one wins the main slot.
    let mut drafts = drafts;
    drafts.sort_by(|a, b| a.start.total_cmp(&b.start));

    // A background vocal is not a line of its own: it is a voice over one.
    let (backgrounds, mains): (Vec<Draft>, Vec<Draft>) =
        drafts.into_iter().partition(|d| d.background);

    let mut lines: Vec<Line> = mains
        .iter()
        .map(|d| Line {
            start: d.start,
            end: f64::INFINITY,
            text: d.text.clone(),
            words: d.words.clone(),
            secondary: Vec::new(),
        })
        .collect();

    attach_backgrounds(&mut lines, backgrounds);
    let mains = collapse_equal_starts(&mut lines, mains);
    assign_ends(&mut lines);
    pair_duets(&mut lines, &mains);
    carry_backgrounds(&mut lines);

    for line in &mut lines {
        close_words(&mut line.words, line.end);
        // A duet partner's words run to when that voice stops, not to when the
        // line it was cut off by takes over.
        for s in &mut line.secondary {
            close_words(&mut s.words, s.end);
        }
        line.secondary.sort_by(|a, b| a.start.total_cmp(&b.start));
    }

    if meta.offset_ms != 0 {
        let shift = meta.offset_ms as f64 / 1000.0;
        for line in &mut lines {
            line.start += shift;
            line.end += shift;
            for w in &mut line.words {
                w.start += shift;
                w.end += shift;
            }
            for s in &mut line.secondary {
                s.start += shift;
                s.end += shift;
                for w in &mut s.words {
                    w.start += shift;
                    w.end += shift;
                }
            }
        }
    }

    Lyrics { lines, meta }
}

/// Fill in every word end still open: the next word's start, or `end` for the
/// last one. An end tag already set one; those are left alone.
fn close_words(words: &mut [Word], end: f64) {
    let count = words.len();
    for w in 0..count {
        if words[w].end.is_finite() {
            continue;
        }
        let next = match w + 1 < count {
            true => words[w + 1].start,
            false => end,
        };
        let w = &mut words[w];
        w.end = next.max(w.start);
    }
}

/// By the `[bg:]` start when given, else the last line already running. One
/// with nowhere to go is dropped: on the wrong line it would be a visible bug.
fn attach_backgrounds(lines: &mut [Line], backgrounds: Vec<Draft>) {
    for bg in backgrounds {
        let target = bg
            .parent
            .and_then(|p| lines.iter().position(|l| (l.start - p).abs() <= SAME_TIME))
            .or_else(|| lines.iter().rposition(|l| l.start <= bg.start));
        let Some(target) = target else { continue };
        let end = bg.sung_end().unwrap_or(bg.start + MIN_SECONDARY_HOLD);
        let kept = unwrap_brackets(&bg.text);
        if kept.is_empty() {
            continue;
        }
        let text: String = bg.text.chars().skip(kept.start).take(kept.len()).collect();
        // The words were measured with the brackets; rebase them, dropping any
        // that was nothing but a bracket.
        let words = bg
            .words
            .iter()
            .filter_map(|w| {
                let start = w.range.start.clamp(kept.start, kept.end) - kept.start;
                let stop = w.range.end.clamp(kept.start, kept.end) - kept.start;
                (stop > start).then(|| Word {
                    range: start..stop,
                    ..w.clone()
                })
            })
            .collect();
        lines[target].secondary.push(Secondary {
            start: bg.start,
            end,
            text,
            words,
            background: true,
        });
    }
}

/// Only a matched pair wrapping the whole phrase: `"(ooh ooh)"` is `ooh ooh`.
/// The dimmed, smaller voice already says what the brackets did.
fn unwrap_brackets(text: &str) -> Range<usize> {
    let chars: Vec<char> = text.chars().collect();
    let trim = |mut r: Range<usize>| {
        while r.start < r.end && chars[r.start].is_whitespace() {
            r.start += 1;
        }
        while r.end > r.start && chars[r.end - 1].is_whitespace() {
            r.end -= 1;
        }
        r
    };
    let outer = trim(0..chars.len());
    if outer.len() < 2 {
        return outer;
    }
    let closer = match (chars[outer.start], chars[outer.end - 1]) {
        ('(', ')') => ')',
        ('（', '）') => '）',
        ('[', ']') => ']',
        _ => return outer,
    };
    let inner = trim(outer.start + 1..outer.end - 1);
    // A pair that closes and reopens midway ("(oh) and (ah)") is not a wrapper.
    match chars[inner.clone()].contains(&closer) {
        true => outer,
        false => inner,
    }
}

/// Such a line would get `end == start` and never be shown. The same text twice
/// is a duplicate, not a second voice. Returns the drafts kept, aligned with
/// `lines`.
fn collapse_equal_starts(lines: &mut Vec<Line>, mains: Vec<Draft>) -> Vec<Draft> {
    let mut kept_lines: Vec<Line> = Vec::with_capacity(lines.len());
    let mut kept_drafts: Vec<Draft> = Vec::with_capacity(mains.len());

    for (line, draft) in std::mem::take(lines).into_iter().zip(mains) {
        let same = kept_lines
            .last()
            .is_some_and(|prev: &Line| (prev.start - line.start).abs() <= SAME_TIME);
        if !same {
            kept_lines.push(line);
            kept_drafts.push(draft);
            continue;
        }

        // The earlier line steps aside; the later one takes the reading position.
        let prev_line = kept_lines.pop().expect("checked by `same`");
        let prev_draft = kept_drafts.pop().expect("aligned with kept_lines");
        let mut line = line;
        if prev_line.text.trim() != line.text.trim() && !prev_line.text.trim().is_empty() {
            line.secondary.push(Secondary {
                start: line.start,
                end: prev_draft.sung_end().unwrap_or(f64::NEG_INFINITY),
                text: prev_line.text.clone(),
                words: prev_line.words.clone(),
                background: false,
            });
        }
        line.secondary.extend(prev_line.secondary);
        kept_lines.push(line);
        kept_drafts.push(draft);
    }

    *lines = kept_lines;
    kept_drafts
}

/// A line runs until the next one starts; its real end never shortens it, which
/// is what makes an overlap a second voice instead of a truncation.
fn assign_ends(lines: &mut [Line]) {
    for idx in 0..lines.len() {
        let next_start = lines.get(idx + 1).map(|l| l.start);
        let line = &mut lines[idx];
        // The last line has no successor; give it a bounded life so the screen
        // clears at the end of the song instead of holding the final phrase.
        line.end = next_start.unwrap_or_else(|| {
            let est = estimate_duration(line.char_len());
            line.start + est
        });
        if line.end < line.start {
            line.end = line.start;
        }
    }
}

/// The later line takes the reading position and the earlier one moves up. Only
/// the preceding line is considered: the screen holds two voices anyway.
fn pair_duets(lines: &mut [Line], mains: &[Draft]) {
    for idx in 1..lines.len() {
        let Some(prev_end) = mains[idx - 1].sung_end() else {
            continue;
        };
        let (prev, cur) = (&lines[idx - 1], &lines[idx]);
        let overlap = prev_end - cur.start;
        if overlap < MIN_DUET_OVERLAP {
            continue;
        }
        let cur_end = mains[idx].sung_end().unwrap_or(cur.end);
        let shorter = (prev_end - prev.start).min(cur_end - cur.start);
        if shorter > 0.0 && overlap < MIN_DUET_OVERLAP_FRACTION * shorter {
            continue;
        }
        let text = prev.text.clone();
        if text.trim().is_empty() {
            continue;
        }
        let words = prev.words.clone();
        let start = cur.start;
        lines[idx].secondary.push(Secondary {
            start,
            end: prev_end,
            text,
            words,
            background: false,
        });
    }
}

/// A backing vocal often runs on after the next line comes in; it goes over to
/// that line too rather than being cut off mid-word.
fn carry_backgrounds(lines: &mut [Line]) {
    let mut carried: Vec<(usize, Secondary)> = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        for s in line.secondary.iter().filter(|s| s.background) {
            let later = lines[idx + 1..]
                .iter()
                .take_while(|l| l.start < s.display_end());
            carried.extend((idx + 1..).zip(later).map(|(to, _)| (to, s.clone())));
        }
    }
    for (to, s) in carried {
        lines[to].secondary.push(s);
    }
}

/// Rough reading time for the final line only, so it does not linger forever.
fn estimate_duration(chars: usize) -> f64 {
    (chars as f64 / 12.0).clamp(2.0, 8.0)
}
