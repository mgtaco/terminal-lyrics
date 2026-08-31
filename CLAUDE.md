# terminal-lyrics

Rust CLI that draws giant block-letter lyrics in the terminal, synced to
whatever the local music player is playing. Crate `terminal-lyrics`, binary
`lyrics`, Rust 2024 edition. Repo: https://github.com/mgtaco/terminal-lyrics
(`origin`, branch `main`). MIT.

Lyrics come from four providers tried in order (AMLL, LyricsPlus, lrcmux,
LRCLIB), with a local `--lrc-dir` and the cache ahead of them. The player is
reached through one seam in `src/player/`, with exactly one backend compiled per
target: MPRIS over D-Bus on Linux, osascript against Spotify and Apple Music on
macOS, a stub on Windows, and a scripted fake for tests.

`config.example.toml` documents every config key; the README explains the
provider ordering and matching in full.

## Design rules worth keeping

- Only word timings end the provider search; a line-level answer is held as a
  fallback and the next provider is still asked. A background vocal's word tags
  do not count — they must not pin a line-level answer in the cache for good.
- A second voice is drawn whole and never swept; the sweep belongs to the line
  being read. `Secondary` carries no `words`, so that is structural.
- Overlap only means a duet past `MIN_DUET_OVERLAP`; below it, one line's tail
  is running into the next, which is most overlap in the wild.
- Word-timed cache entries are kept indefinitely; line-level ones expire after a
  day, same as a miss.
- Player picking stays platform-neutral: playing beats has-a-track beats
  anything else, name as tiebreak.
- Nothing interpolated (e.g. the sweep highlight) is ever written to disk.
- The sweep says which word of the line is being sung, so `Sweep::Auto` wants
  both halves of that: real word timings, and the rest of the line to point at.
  One word at a time supplies the answer on its own, so auto turns itself off
  there. `always` still sweeps the syllables of the word on screen.
- The sync nudge is per track, keyed by the same id as the lyrics cache, and
  saved under the data dir rather than the cache dir — it is the user's own
  correction, and a cache is by definition safe to delete. Only tuned tracks get
  an entry: `config.offset_ms` is the starting point for the rest, and a song
  reset to it is deleted rather than stored.
- Tests must never shell out to `osascript` — it would prompt for Automation
  access. Keep macOS decisions in `parse_probe`.

## Working on it

```bash
cargo test                              # 182 tests, no terminal or player needed
cargo clippy --all-targets -- -D warnings
cargo run --example pump_dump -- 15     # dump the live player event stream
lyrics status                           # what it sees: player, track, source
```

The pure parts — the parser, timeline, clock, config layering, layout and match
scoring — sit behind the library target, so they test without a bus or a TTY.
For what cannot be pure, `src/player/fake.rs` replays a scripted event timeline,
which drives the sync engine at arbitrary "times" without any sleeping. The
macOS backend is split the same way: everything with a decision in it is in
`parse_probe`, leaving only the `osascript` spawn untested.

## After making changes

Both of these, every time a change works:

1. **Build and install**, so the `lyrics` on PATH has the change:

   ```bash
   cargo build --release
   install -m755 target/release/lyrics ~/.local/bin/lyrics
   ```

   (`install -D` is Linux-only; macOS's `-D` means something else, so make the
   directory separately if it is missing.)

2. **Commit and push** to `origin main`.
