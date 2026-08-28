//! One LRC parser.
//!
//! Two parsers with subtly different regexes is an easy mistake to make and a
//! miserable one to debug: lines that write out fine become invisible on
//! playback. There is one parser here and it handles the whole dialect:
//!
//! * `[mm:ss]`, `[mm:ss.xx]`, `[mm:ss.xxx]` — fractional seconds optional
//! * multiple timestamps on one line: `[00:10.00][01:20.00]same words`
//! * `[offset:+250]` / `[offset:-250]`, applied to every timestamp
//! * ID tags (`ti`, `ar`, `al`, `by`, `length`) kept as metadata, not lyrics
//! * enhanced (A2) word tags: `[00:12.00]<00:12.00>Hello <00:12.50>world`
//! * `#` comments and blank lines
//!
//! Timestamped lines with no text are kept: in a well-made LRC they mark
//! instrumental gaps, and dropping them leaves the previous line frozen on
//! screen through the whole break.

use std::ops::Range;

/// A timed span from an enhanced-LRC source: one word, or one syllable of a
/// word when the source times long words in pieces. [`Line::continues_word`]
/// tells the two apart.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub start: f64,
    /// When the word stops being highlighted. A trailing `<mm:ss.xx>` tag sets
    /// this explicitly; otherwise finalisation fills in the next word's start.
    /// The difference matters when a source records a real gap between words —
    /// without it the highlight smears across the pause.
    pub end: f64,
    /// Char range of this word inside [`Line::text`], for highlighting.
    pub range: Range<usize>,
}

/// A second voice singing over a line: a background vocal ("(ooh ooh)"), or the
/// other half of a duet whose phrase is still running when this line comes in.
///
/// It carries no `words`. A second voice is drawn whole and never swept — the
/// sweep belongs to the line being read — and leaving the field out makes that
/// a property of the model rather than a rule someone has to remember.
#[derive(Debug, Clone, PartialEq)]
pub struct Secondary {
    /// When it comes in. Not the host line's start: a background phrase usually
    /// begins partway through the line it sits over.
    pub start: f64,
    /// When the source stops it.
    pub end: f64,
    pub text: String,
    /// Background vocals are dimmed and drawn a size smaller. The other half of
    /// a duet is neither: it is a co-equal voice.
    pub background: bool,
}

/// How long a second voice stays up once it has appeared.
///
/// Apple times a background phrase to the syllable, so "(ooh)" can be a fifth
/// of a second long. Drawn and pulled that fast it reads as a flicker rather
/// than as a voice. Display-only, derived on read — never written to disk.
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

    /// Whether word `i` continues the word before it instead of starting a new
    /// one — a syllable rather than a word.
    ///
    /// Word-timed sources time long words in pieces (`be` then `lieve`), and the
    /// only thing that separates two real words is the whitespace between them.
    /// A timed span butted straight up against the one before it is therefore
    /// part of the same word, and must not be shown as a word of its own.
    pub fn continues_word(&self, i: usize) -> bool {
        let Some(prev) = i.checked_sub(1).and_then(|p| self.words.get(p)) else {
            return false;
        };
        let Some(cur) = self.words.get(i) else {
            return false;
        };
        let gap = cur.range.start.saturating_sub(prev.range.end);
        self.text
            .chars()
            .skip(prev.range.end)
            .take(gap)
            .all(|c| !c.is_whitespace())
    }

    /// The char range of the whole word that word `i` belongs to, from the start
    /// of its first syllable to the end of its last.
    pub fn word_bounds(&self, i: usize) -> Option<Range<usize>> {
        self.words.get(i)?;
        let mut first = i;
        while self.continues_word(first) {
            first -= 1;
        }
        let mut last = i;
        while self.continues_word(last + 1) {
            last += 1;
        }
        Some(self.words[first].range.start..self.words[last].range.end)
    }
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

/// `mm:ss`, `mm:ss.xx`, `hh:mm:ss.xx` → seconds. Rejects anything else, which is
/// what keeps `[ti:Song]` from being read as a time. Shared with the TTML
/// converter, which uses the same clock syntax.
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

/// Record a word spanning `from..to` (char indices), skipping the whitespace
/// that separates it from the next one so the highlight lands on the word.
///
/// A tag with no visible text behind it is an *end* tag: it closes the previous
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

/// The marker groups a line may carry between its timestamps and its text.
///
/// `[bg:mm:ss.SSS]` marks the line as a second voice and names the line it
/// sings over; `[end:mm:ss.SSS]` records when the source says the singing
/// actually stops. Both sit in the one place the format leaves free: a bracket
/// group that is not a clock stops [`parse_timestamp`]'s loop, so the slot was
/// already unreachable, and putting them ahead of the body means every word's
/// char range is still measured from the first character of the text.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Markers {
    background: bool,
    /// Start timestamp of the line this one sings over. A parent pointer, not a
    /// duration.
    parent: Option<f64>,
    end: Option<f64>,
}

/// Read the marker groups at `i`, stopping at the first bracket group that is
/// not one. Anything else is left exactly where it was and becomes body text,
/// so `[00:10.00][verse 1]words` still reads as the words `[verse 1]words`.
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

/// One parsed line before overlap and gaps have been worked out.
///
/// Private on purpose: `real_end` and the background marker say what the source
/// wrote, and mean nothing once [`finalise`] has turned them into
/// [`Line::secondary`]. In particular `real_end` is not [`Line::end`] — one is
/// when the singing stops, the other is when the line leaves the screen.
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

/// How long two lines must overlap, and by what share of the shorter of them,
/// before they count as two voices singing at once rather than as one line's
/// tail bleeding into the next line's head.
///
/// Measured over 39 files from the AMLL database — 1863 `<p>` elements, 95
/// overlapping consecutive pairs. The overlaps run: min 0.00 s, p25 0.06 s,
/// median 0.25 s, p75 0.93 s, max 4.42 s. 61% are under half a second and 76%
/// under a second, which is timing slop, not a duet; stacking those would put a
/// second line on screen roughly two and a half times a song for a quarter of a
/// second each, and read as flicker rather than as a second singer. At these
/// values 20 of the 95 qualify — about one duet moment every two songs.
///
/// `ttm:agent` deliberately plays no part in the test: 42 of those 95 pairs
/// share an agent, so who is singing cannot tell the two cases apart. Only the
/// times can.
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

/// Sort, work out who is singing over whom, fill in every `end`, and apply
/// `[offset:]`, so lookup never has to reason about neighbours.
///
/// All of the structural work happens in the file's own coordinates and the
/// offset is applied once at the very end. That is simpler than shifting as we
/// go, and it is why every `end` here — line, word and second voice alike —
/// moves with the shift instead of only the starts.
fn finalise(drafts: Vec<Draft>, meta: Meta) -> Lyrics {
    // Multi-timestamp lines arrive out of order by construction. The sort is
    // stable, so two lines that begin together keep the order they were written
    // in and the later one wins the main slot.
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

    for line in &mut lines {
        let line_end = line.end;
        let count = line.words.len();
        for w in 0..count {
            // An end tag already set this; only fill in the ones still open.
            if line.words[w].end.is_finite() {
                continue;
            }
            let end = if w + 1 < count {
                line.words[w + 1].start
            } else {
                line_end
            };
            let w = &mut line.words[w];
            w.end = end.max(w.start);
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
            }
        }
    }

    Lyrics { lines, meta }
}

/// Hang each background vocal on the line it sings over.
///
/// The `[bg:]` marker names that line by its start, which is exact where the
/// source gave us one. Without it, the last line already running is the only
/// sensible guess. A background with nowhere to go is dropped: it is texture,
/// and one hung on the wrong line is a visible bug, while a missing one costs
/// nothing.
fn attach_backgrounds(lines: &mut [Line], backgrounds: Vec<Draft>) {
    for bg in backgrounds {
        let target = bg
            .parent
            .and_then(|p| lines.iter().position(|l| (l.start - p).abs() <= SAME_TIME))
            .or_else(|| lines.iter().rposition(|l| l.start <= bg.start));
        let Some(target) = target else { continue };
        if bg.text.trim().is_empty() {
            continue;
        }
        let end = bg.sung_end().unwrap_or(bg.start + MIN_SECONDARY_HOLD);
        lines[target].secondary.push(Secondary {
            start: bg.start,
            end,
            text: bg.text,
            background: true,
        });
    }
}

/// Fold away lines that share a timestamp, keeping the last as the line and the
/// earlier ones as voices over it.
///
/// Such a line has no window of its own: `end` would come out equal to `start`
/// and [`crate::timeline::Timeline::locate`] could never return it, so it was
/// invisible. Two singers given the same timestamp is exactly the case this
/// whole feature is about, so it is folded rather than dropped — unless the
/// text is the same, which is a duplicated line rather than a second voice and
/// would only stack a phrase on top of itself.
///
/// Returns the surviving drafts, aligned with `lines`.
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

/// Give every line the window it holds the screen for.
///
/// Unchanged in spirit: a line runs until the next one starts, so it stays up
/// through the gap rather than blinking out. A line's real end never shortens
/// it — that is what makes an overlap a second voice instead of a truncation.
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

/// Where one line is still being sung as the next comes in, put it above the
/// new one instead of cutting it off.
///
/// The later line takes the reading position and the earlier one moves up: the
/// alternative leaves the newly-arrived voice hidden behind a phrase that is
/// already finishing. Nothing is lost by copying the text — the earlier line
/// keeps its own window, its words and its sweep right up to the moment the
/// second voice arrives.
///
/// Only the immediately preceding line is considered. A three-way overlap is
/// vanishingly rare and the screen holds two voices anyway.
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
        let start = cur.start;
        lines[idx].secondary.push(Secondary {
            start,
            end: prev_end,
            text,
            background: false,
        });
    }
}

/// Rough reading time for the final line only, so it does not linger forever.
fn estimate_duration(chars: usize) -> f64 {
    (chars as f64 / 12.0).clamp(2.0, 8.0)
}
