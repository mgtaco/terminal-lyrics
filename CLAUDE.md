# terminal-lyrics

Rust CLI that draws giant block-letter lyrics in the terminal, synced to
whatever the local music player is playing. Crate `terminal-lyrics`, binary
`lyrics`, Rust 2024 edition. Repo: https://github.com/mgtaco/terminal-lyrics
(`origin`, branch `main`). MIT.

The lyrics lookup in `src/lyrics/` is a port of DankMaterialShell's Go library
(`github.com/AvengeMedia/dankgo/lyrics`), so that `lyrics` and the DMS widget
pick the same lyrics: six providers (BetterLyrics, Unison, LyricsPlus, KuGou,
LRCLIB, YouTube Music) raced at once, with `--lrc-dir` and a sidecar file next
to a local track ahead of them, and a cache shared with DMS. The player is
reached through one seam in `src/player/`, with exactly one backend compiled per
target: MPRIS over D-Bus on Linux, osascript against Spotify and Apple Music on
macOS, a stub on Windows, and a scripted fake for tests.

`config.example.toml` documents every config key; the README explains the
provider ordering and matching in full.

## Design rules worth keeping

- The lookup's job is to agree with DMS, so `src/lyrics/{model,engine,cache,
  sidecar,format,providers}` follow dankgo's behaviour, request shape and
  cache format exactly; a "better" rule added only here makes the two pick
  different lyrics. Change those by porting the change from dankgo. The DMS
  request uses all artists joined with ", " and the length rounded to seconds,
  which is why MPRIS artists are joined that way.
- The cache format is DMS's on-disk format (`lyrics/v4` keys, one JSON entry per
  provider per track), written atomically, because the two programs read and
  write the same directory when `~/.cache/DankMaterialShell` exists.
- Everything past the lookup consumes enhanced LRC: `display::to_lrc` turns the
  provider model into the `[bg:]`/word-tag form `crate::lrc` parses, so the
  duet, background and sweep logic is shared by every source.
- A second voice is drawn whole and never swept; the sweep belongs to the line
  being read. `Secondary` carries no `words`, so that is structural.
- Overlap only means a duet past `MIN_DUET_OVERLAP`; below it, one line's tail
  is running into the next, which is most overlap in the wild.
- Player picking stays platform-neutral: playing beats has-a-track beats
  anything else, name as tiebreak.
- Nothing interpolated (e.g. the sweep highlight) is ever written to disk.
- The sweep says which word of the line is being sung, so `Sweep::Auto` wants
  both halves of that: real word timings, and the rest of the line to point at.
  One word at a time supplies the answer on its own, so auto turns itself off
  there. `always` still sweeps the syllables of the word on screen.
- The sync nudge is per track, keyed by `Track::id` (not the lyrics cache key), and
  saved under the data dir rather than the cache dir — it is the user's own
  correction, and a cache is by definition safe to delete. Only tuned tracks get
  an entry: `config.offset_ms` is the starting point for the rest, and a song
  reset to it is deleted rather than stored.
- Tests must never shell out to `osascript` — it would prompt for Automation
  access. Keep macOS decisions in `parse_probe`.

## Working on it

```bash
cargo test                              # 189 tests, no terminal or player needed
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
