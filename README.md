# terminal-lyrics

Giant block lyrics in your terminal, synced to whatever is playing.

```
 ███████  █████        █████    ███   ███████ ██   ██ ███████ ██████
   ██    ██   ██      ██       ██ ██    ██    ██   ██ ██      ██   ██
   ██    ██   ██      ██  ███ ███████   ██    ███████ █████   ██████
   ██    ██   ██      ██   ██ ██   ██   ██    ██   ██ ██      ██  ██
   ██     █████        █████  ██   ██   ██    ██   ██ ███████ ██   ██
```

Run one command, play something, lyrics appear. No music library to scan, no
pre-processing step, no Python environment.

## Install

```bash
cargo build --release
install -Dm755 target/release/lyrics ~/.local/bin/lyrics
```

That is the whole install. The binary is self-contained: D-Bus is spoken
directly and TLS is pure Rust, so there is nothing to install alongside it.

## Use

```bash
lyrics
```

It finds your MPRIS player, reads what is playing, downloads synced lyrics from
[LRCLIB](https://lrclib.net), and displays them a phrase at a time in block
letters, with a highlight sweeping across each line as it is sung.

| key | |
|---|---|
| `q` / `Esc` | quit |
| `space` | play/pause the player |
| `,` / `.` | shift lyrics 100 ms earlier / later |
| `0` | reset the shift |
| `f` | cycle font (block → compact → mini) |
| `s` | cycle the word highlight: auto → always → never |
| `r` | forget the cached lyrics and look them up again |

Useful flags: `--sweep` to force the word highlight on even for line-level
lyrics, `--no-sweep` to force it off, `--player spotify` to pin a player when
several are running, `--font compact`, `--offset-ms -250` when a particular LRC file is
timed badly, `--lrc-dir ~/lyrics` to prefer your own `Artist - Title.lrc` files,
`--no-network` to use only those and the cache.

## Without the TUI

```bash
lyrics status                 # player, track, position, and which source matched
lyrics fetch --artist "Radiohead" --title "Creep" --duration 238
lyrics paths                  # where the config and cache live
```

`status` is the first thing to run when something is not working: it prints the
resolved player, the track it sees, the length the player reported, the cache
key, and where the lyrics came from.

## Configuration

`~/.config/terminal-lyrics/config.toml`; see `config.example.toml` for every
key. Flags override the file, the file overrides the defaults, and there is a
test asserting that per field.

## How it works

**Position.** The player is asked once, and after that the position is
interpolated from a monotonic clock. Property changes and `Seeked` re-anchor it;
a 1 Hz `Position` read catches players that seek without saying so — Spotify
among them. No subprocesses.

**Lookup.** `--lrc-dir` first, then the cache, then LRCLIB: an exact `/api/get`
with artist, title, album and duration, then without the album, then a scored
`/api/search`, then a retry with `- Remastered 2011` and `(feat. …)` stripped.
Candidates more than five seconds from the track's real length are rejected
rather than shown out of sync. Misses are cached for a day so a track LRCLIB has
never heard of is not re-queried on every play.

**Word timing.** The word highlight follows the lyrics, not a preference. By
default (`sweep = "auto"`) it appears only for sources that carry real per-word
timestamps, and stays off for line-level ones — where moving it would mean
animating a guess. `--sweep` forces it on anyway, interpolating across the
phrase between its two real timestamps weighted by character count; `--no-sweep`
forces it off. `lyrics status` says which you have and whether the highlight
will show. Either way it is only a display effect, and nothing invented is
written to disk.

**Drawing.** ratatui diffs the screen buffer, so an unchanged frame emits no
bytes. Lines wrap at word boundaries and the font steps down before anything is
clipped; nothing is ever truncated.

## Development

```bash
cargo test                      # 61 tests, no terminal or player needed
cargo clippy --all-targets -- -D warnings
cargo run --example pump_dump -- 15   # dump the live player event stream
```

The pure parts — parser, timeline, clock, config layering, layout, match
scoring — live behind a library target and are tested without a bus or a TTY.
`src/player/fake.rs` replays a scripted event timeline so the sync engine can be
driven at arbitrary "times" without sleeping.

## Limitations

* Lyrics come from LRCLIB. Anything not in it will not be found, and `r` will
  not conjure it.
* Most LRCLIB entries are line-level, so the within-line sweep is usually an
  interpolation rather than real word timing. `lyrics status` says which you got.
* MPRIS only. A player that does not expose an MPRIS interface is invisible.

## License

MIT
