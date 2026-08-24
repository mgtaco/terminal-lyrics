<div align="center">

# terminal-lyrics

**Giant block lyrics in your terminal, synced to whatever is playing.**

[![Rust 2024 edition](https://img.shields.io/badge/Rust-2024_edition-B7410E?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![MIT licence](https://img.shields.io/github/license/mgtaco/terminal-lyrics?style=flat-square&color=8A2BE2)](LICENSE)
[![Linux, MPRIS over D-Bus](https://img.shields.io/badge/Linux-MPRIS_%2F_D--Bus-FCC624?style=flat-square&logo=linux&logoColor=black)](#limitations)
[![Self-contained binary](https://img.shields.io/badge/binary-self--contained-2EA44F?style=flat-square)](#install)
[![Lyrics from LRCLIB and AMLL](https://img.shields.io/badge/lyrics-LRCLIB_%2B_AMLL-0EA5E9?style=flat-square)](#how-it-works)
[![Code size](https://img.shields.io/github/languages/code-size/mgtaco/terminal-lyrics?style=flat-square&color=64748B)](https://github.com/mgtaco/terminal-lyrics)

![Word-timed lyrics appearing one word at a time, a long word building up syllable by syllable](docs/demo.gif)

<sub>Rendered by the program itself. The demo words were written for it, so what
you see is the display, not anyone's lyrics.</sub>

</div>

Run one command, play something, and the lyrics appear. There is no music
library to scan, no pre-processing pass to sit through, and no Python
environment to build first.

## Install

```bash
cargo build --release
install -Dm755 target/release/lyrics ~/.local/bin/lyrics
```

That is the whole install, because the binary is self-contained: it speaks
D-Bus directly and its TLS is pure Rust, so nothing needs installing alongside
it.

## Use

```bash
lyrics
```

It finds your MPRIS player, reads what is playing, downloads synced lyrics and
draws them in block letters. Where the lyrics carry real per-word timings they
appear **one word at a time**, each word arriving as it is sung, while
line-level lyrics are shown a phrase at a time.

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

A few flags are worth knowing. `--whole-lines` always shows the full phrase,
`--sweep` highlights the sung part of whatever is on screen, and `--font
compact` picks a smaller face. If several players are running, `--player
spotify` pins the one you mean, and `--offset-ms -250` rescues a file that was
timed badly. To work from your own lyrics rather than the network,
`--lrc-dir ~/lyrics` prefers your `Artist - Title.lrc` files, and `--no-network`
restricts it to those and the cache.

## Without the TUI

```bash
lyrics status                 # player, track, position, and which source matched
lyrics fetch --artist "Radiohead" --title "Creep" --duration 238
lyrics fetch --artist "Kanye West" --title "Flashing Lights" \
             --spotify-id 5TRPicyLGbAF2LGBFbHGvO   # word-timed, via AMLL
lyrics paths                  # where the config and cache live
```

When something is not working, `status` is the place to start, since it prints
every player and what each one is doing, along with the track it settled on, the
length the player reported, the cache key, whether the lyrics carry real word
timings and where they came from. The strip along the bottom of the visualiser
shows only that last part, the source, so that it stays out of the way of the
lyrics.

## Configuration

Settings live in `~/.config/terminal-lyrics/config.toml`, and
`config.example.toml` lists every key. Flags override the file and the file
overrides the defaults, with a test asserting that ordering field by field.

## How it works

**Choosing a player.** Without `--player`, whichever player is actually playing
wins, followed by one with a track loaded and then anything else, with the bus
name as a stable tiebreak. Simply taking the first name alphabetically turns out
to be wrong in practice, because browsers register idle MPRIS instances such as
`chromium.instance26065` that sort ahead of `spotify` while reporting no track
at all. If the wrong one is being picked, `lyrics status` lists what each of
them is doing.

**Position.** The player is asked once and the position is interpolated from a
monotonic clock from then on, re-anchoring whenever a property changes or a
`Seeked` signal arrives. A 1 Hz `Position` read covers the players that seek
without announcing it, Spotify among them, and none of this costs a
subprocess.

**Sources.** `--lrc-dir` first, then the cache, then two networks in order:

* the [AMLL TTML database](https://github.com/amll-dev/amll-ttml-db), which is
  CC0, community-maintained and **word-by-word**, and which is keyed by the
  Spotify track ID the player already hands us, so it needs neither a search nor
  any matching guesswork. It holds only about 2,400 entries, so it is tried
  first and misses often.
* [LRCLIB](https://lrclib.net), which has millions of tracks but line-level
  timing only. Every one of the 923 `syncedLyrics` entries sampled was
  line-level, which is why the word-by-word display depends on the first source
  being worth having.

**Matching LRCLIB.** The lookup starts with an exact `/api/get` on artist,
title, album and duration, retries without the album, falls back to a scored
`/api/search`, and finally tries once more with decorations like
`- Remastered 2011` and `(feat. …)` stripped from the title. Candidates more
than five seconds away from the track's real length are rejected rather than
displayed out of sync, and misses are cached for a day so that a track LRCLIB
has never heard of is not queried again on every play.

**Word timing.** What appears on screen follows the lyrics rather than a
preference. A source carrying real per-word timestamps, which in practice means
an AMLL hit, is shown one word at a time with each word appearing on its own
timestamp, whereas a line-level source has nothing to split on and so is shown
whole. Because that decision is made per line rather than per file, a file that
carries tags on only some of its lines still shows the rest in full. Where a
long word is timed in pieces, the pieces build the word up in place instead of
flashing a syllable on its own.

**Highlight.** The highlight is a separate thing and is off unless you ask for
it. `--sweep` lights up the sung part of whatever is on screen, moving across
the current word in word-by-word mode and across the phrase otherwise. For
line-level lyrics it has to be interpolated from character counts between two
real timestamps, which makes it an estimate, and `lyrics status` labels it as
one. It remains a display effect throughout: nothing invented is ever written to
disk.

**Drawing.** ratatui diffs the screen buffer, so a frame that has not changed
emits no bytes at all. Lines wrap at word boundaries and the font steps down a
size before anything would be clipped, so nothing is ever truncated.

## Development

```bash
cargo test                      # 95 tests, no terminal or player needed
cargo clippy --all-targets -- -D warnings
cargo run --example pump_dump -- 15   # dump the live player event stream
```

The pure parts — the parser, timeline, clock, config layering, layout and match
scoring — sit behind a library target so they can be tested without a bus or a
TTY. For the parts that cannot be pure, `src/player/fake.rs` replays a scripted
event timeline, which lets the sync engine be driven at arbitrary "times"
without any sleeping.

## Limitations

* Word-timed lyrics only exist for what is in the AMLL database — about 2,400
  entries, 1,588 distinct songs once the duplicate releases are folded together.
  Everything else falls back to LRCLIB's line-level entries and shows no moving
  highlight. `lyrics status` says which you got. To hear the word-by-word mode
  without hunting for a match, this playlist collects the English-language ones:
  [AMLL word-timed (English)](https://open.spotify.com/playlist/73AC0u1Ujko0IpNFnRxzAo),
  429 songs. It is only the English slice — the database is around half Chinese
  and Japanese, and those work just as well.
* Because AMLL is keyed by Spotify track ID, the word-timed path only applies
  when the player reports one, though other players still get LRCLIB.
* Anything held by neither source will not be found, and pressing `r` will not
  conjure it into existence.
* Only MPRIS players are visible, so anything that does not expose an MPRIS
  interface goes unnoticed.

## Credit

The idea and the big block-letter look come from
[tacos-terminal-lyrics](https://github.com/tacoproz1/tacos-terminal-lyrics) by
tacoproz1. This is a separate program written from scratch in Rust rather than a
fork of that one, and it takes a different approach, with no music library to
scan and no pre-processing pass.

Lyrics come from [LRCLIB](https://lrclib.net) and the
[AMLL TTML database](https://github.com/amll-dev/amll-ttml-db) (CC0), neither of
which is affiliated with this project.

## License

MIT
