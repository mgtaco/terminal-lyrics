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

It finds your MPRIS player, reads what is playing, downloads synced lyrics, and
displays them in block letters. Lyrics that carry real per-word timings are
shown **one word at a time**, each word appearing as it is sung; line-level
lyrics are shown a phrase at a time.

| key | |
|---|---|
| `q` / `Esc` | quit |
| `space` | play/pause the player |
| `,` / `.` | shift lyrics 100 ms earlier / later |
| `0` | reset the shift |
| `f` | cycle font (block → compact → mini) |
| `w` | switch between one word at a time and whole lines |
| `s` | cycle the highlight: never → auto → always |
| `r` | forget the cached lyrics and look them up again |

Useful flags: `--whole-lines` to always show the full phrase, `--sweep` to
highlight the sung part of what is on screen (off by default), `--player
spotify` to pin a player when several are running, `--font compact`, `--offset-ms -250` when a particular LRC file is
timed badly, `--lrc-dir ~/lyrics` to prefer your own `Artist - Title.lrc` files,
`--no-network` to use only those and the cache.

## Without the TUI

```bash
lyrics status                 # player, track, position, and which source matched
lyrics fetch --artist "Radiohead" --title "Creep" --duration 238
lyrics fetch --artist "Kanye West" --title "Flashing Lights" \
             --spotify-id 5TRPicyLGbAF2LGBFbHGvO   # word-timed, via AMLL
lyrics paths                  # where the config and cache live
```

`status` is the first thing to run when something is not working: it prints
every player and what it is doing, the track it sees, the length the player
reported, the cache key, whether the lyrics carry real word timings, and where
they came from. The strip along the bottom of the visualiser deliberately shows
only that last part — the source — so it stays out of the way of the lyrics.

## Configuration

`~/.config/terminal-lyrics/config.toml`; see `config.example.toml` for every
key. Flags override the file, the file overrides the defaults, and there is a
test asserting that per field.

## How it works

**Choosing a player.** With no `--player`, the one that is actually playing
wins, then one with a track loaded, then anything else, with the name as a
stable tiebreak. Taking the first name alphabetically is wrong in practice:
browsers register idle MPRIS instances (`chromium.instance26065`) that sort
before `spotify` and report no track. `lyrics status` lists what each player is
doing.

**Position.** The player is asked once, and after that the position is
interpolated from a monotonic clock. Property changes and `Seeked` re-anchor it;
a 1 Hz `Position` read catches players that seek without saying so — Spotify
among them. No subprocesses.

**Sources.** `--lrc-dir` first, then the cache, then two networks in order:

* the [AMLL TTML database](https://github.com/amll-dev/amll-ttml-db) — CC0,
  community-maintained, **word-by-word**, and keyed by the Spotify track ID the
  player already hands us, so it needs no search and no matching guesswork.
  Around 2,400 tracks, so it is tried first and misses often.
* [LRCLIB](https://lrclib.net) — millions of tracks, line-level only. Every
  `syncedLyrics` entry sampled (923 of them) was line-level, which is why the
  word highlight needs the first source to be worth having.

**Matching LRCLIB.** An exact `/api/get` with artist, title, album and duration,
then without the album, then a scored `/api/search`, then a retry with
`- Remastered 2011` and `(feat. …)` stripped.
Candidates more than five seconds from the track's real length are rejected
rather than shown out of sync. Misses are cached for a day so a track LRCLIB has
never heard of is not re-queried on every play.

**Word timing.** What is on screen follows the lyrics, not a preference. A
source with real per-word timestamps — in practice an AMLL hit — is shown one
word at a time, each word appearing on its own timestamp. A line-level source
has nothing to split on, so it is shown whole. The decision is made per line, so
a file carrying tags on only some lines still shows the rest in full.

**Highlight.** Separate, and off by default. `--sweep` highlights the sung part
of whatever is on screen: across the current word in word-by-word mode, across
the phrase otherwise. For line-level lyrics that highlight is interpolated from
character counts between two real timestamps — an estimate, and `lyrics status`
labels it as one. It is only ever a display effect; nothing invented is written
to disk.

**Drawing.** ratatui diffs the screen buffer, so an unchanged frame emits no
bytes. Lines wrap at word boundaries and the font steps down before anything is
clipped; nothing is ever truncated.

## Development

```bash
cargo test                      # 90 tests, no terminal or player needed
cargo clippy --all-targets -- -D warnings
cargo run --example pump_dump -- 15   # dump the live player event stream
```

The pure parts — parser, timeline, clock, config layering, layout, match
scoring — live behind a library target and are tested without a bus or a TTY.
`src/player/fake.rs` replays a scripted event timeline so the sync engine can be
driven at arbitrary "times" without sleeping.

## Limitations

* Word-timed lyrics only exist for what is in the AMLL database — a couple of
  thousand tracks. Everything else falls back to LRCLIB's line-level entries and
  shows no moving highlight. `lyrics status` says which you got.
* AMLL is keyed by Spotify track ID, so the word-timed path only applies when
  the player reports one. Other players still get LRCLIB.
* Anything in neither source will not be found, and `r` will not conjure it.
* MPRIS only. A player that does not expose an MPRIS interface is invisible.

## Credit

The idea and the big block-letter look come from
[tacos-terminal-lyrics](https://github.com/tacoproz1/tacos-terminal-lyrics) by
tacoproz1. This is a separate program written from scratch in Rust, not a fork
of it, and it takes a different approach: no music library to scan and no
pre-processing pass.

Lyrics come from [LRCLIB](https://lrclib.net) and the
[AMLL TTML database](https://github.com/amll-dev/amll-ttml-db) (CC0). Neither is
affiliated with this project.

## License

MIT
