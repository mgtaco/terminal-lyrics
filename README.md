<div align="center">

# terminal-lyrics

**Giant block lyrics in your terminal, synced to whatever is playing.**

[![Rust 2024 edition](https://img.shields.io/badge/Rust-2024_edition-B7410E?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![MIT licence](https://img.shields.io/github/license/mgtaco/terminal-lyrics?style=flat-square&color=8A2BE2)](LICENSE)
[![Linux and macOS](https://img.shields.io/badge/Linux_%2B_macOS-MPRIS_%2F_AppleScript-64748B?style=flat-square)](#platforms)
[![Self-contained binary](https://img.shields.io/badge/binary-self--contained-2EA44F?style=flat-square)](#install)
[![Four lyrics sources](https://img.shields.io/badge/lyrics-4_synced_sources-0EA5E9?style=flat-square)](#how-it-works)
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
mkdir -p ~/.local/bin && install -m755 target/release/lyrics ~/.local/bin/lyrics
```

(`install -D` would make the directory for you, but only on Linux: the `-D` that
macOS ships is a different flag entirely.)

That is the whole install, because the binary is self-contained: it links its
TLS in statically and talks to your player through whatever the platform already
provides, so nothing needs installing alongside it. The only shared libraries it
wants are the C runtime, which is already on your system.

On macOS the first run raises the standard Automation prompt, because reading
what Spotify is playing means sending it an Apple event. Allow it once and it
stops asking. If you refuse and change your mind, the switch is under System
Settings → Privacy & Security → Automation.

## Use

```bash
lyrics
```

It finds your player, reads what is playing, downloads synced lyrics and
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

Settings live in `~/.config/terminal-lyrics/config.toml`, or
`~/Library/Application Support/terminal-lyrics/config.toml` on macOS; `lyrics
paths` prints the one this build actually reads. `config.example.toml` lists
every key. Flags override the file and the file overrides the defaults, with a
test asserting that ordering field by field.

## How it works

**Choosing a player.** Without `--player`, whichever player is actually playing
wins, followed by one with a track loaded and then anything else, with the name
as a stable tiebreak. Simply taking the first name alphabetically turns out to be
wrong in practice, because browsers register idle MPRIS instances such as
`chromium.instance26065` that sort ahead of `spotify` while reporting no track
at all. That ranking is platform-neutral and shared by both backends. If the
wrong one is being picked, `lyrics status` lists what each of them is doing.

**Position.** The player is asked once and the position is interpolated from a
monotonic clock from then on, re-anchoring whenever the player says something.
A 1 Hz position read covers the players that seek without announcing it, Spotify
among them. On Linux that read costs no subprocess at all; on macOS it is one
short-lived `osascript`, which is also what makes a scrub show up within a
second there despite AppleScript having no seek notification to offer.

**Sources.** `--lrc-dir` first, then the cache, then four networks in order.
The order is by what each is good at rather than by how often it answers, so a
better-looking set of lyrics is never passed over for a merely likelier one:

* the [AMLL TTML database](https://github.com/amll-dev/amll-ttml-db), which is
  CC0, community-maintained and **syllable-timed**, and which is keyed by the
  Spotify track ID the player already hands us, so it needs neither a search nor
  any matching guesswork. It holds only about 2,400 entries, so it costs one
  request and misses often — but when it hits, nothing else needs asking.
* [LyricsPlus](https://github.com/ibratabian17/lyricsplus), which serves Apple
  Music's own TTML and is **syllable-timed** too: it splits a long word into
  pieces the display then builds up in place. Matched on artist and title, with
  the length Apple records in the document used to throw out a different edit.
  Its `platformId=spotify:…` parameter is not used: the public instance has no
  Spotify credentials to resolve an ID with, and answers every such query 404.
* [lrcmux](https://github.com/f1nniboy/lrcmux), one API in front of Musixmatch
  richsync, KuGou, NetEase, Genius and YouTube Music. **Word-timed**, and the
  widest of the four: five upstreams means it degrades rather than dies. It says
  outright whether the answer it found carries word timings.
* [LRCLIB](https://lrclib.net), which has millions of tracks but line-level
  timing only. Every one of the 923 `syncedLyrics` entries sampled was
  line-level, which is why it is asked last rather than first.

LyricsPlus and lrcmux are small community-run instances, and both projects
document self-hosting, so both base URLs are configurable — as is the list
itself, which doubles as the on/off switch:

```toml
providers = ["amll", "lyricsplus", "lrcmux", "lrclib"]
```

Dropping a name skips that provider; the order of the list is the order they are
consulted in. A provider that is down is logged and stepped over, because losing
LRCLIB's line-level answer to somebody's server being unreachable would be the
worst trade available. A lookup only fails outright when every provider failed
and none of them managed to say "not here".

**Matching LRCLIB.** The lookup starts with an exact `/api/get` on artist,
title, album and duration, retries without the album, falls back to a scored
`/api/search`, and finally tries once more with decorations like
`- Remastered 2011` and `(feat. …)` stripped from the title. Candidates more
than five seconds away from the track's real length are rejected rather than
displayed out of sync, and misses are cached for a day so that a track LRCLIB
has never heard of is not queried again on every play.

**Word timing.** What appears on screen follows the lyrics rather than a
preference. A source carrying real per-word timestamps — three of the four now
do — is shown one word at a time with each word appearing on its own
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

## Platforms

The program is the same everywhere; only the way it finds your player differs,
and that lives behind one seam in `src/player/`. Exactly one backend is compiled
into a given binary, so a Linux package never carries the macOS code or the other
way round.

**Linux** talks MPRIS over D-Bus, directly, with no subprocess anywhere: property
changes and `Seeked` arrive as signals. Anything exposing an MPRIS interface
works, which is most things.

**macOS** drives Spotify and Apple Music through their scripting dictionaries,
polled once a second. It knows those two by name rather than discovering players
generally, because there is nothing left to discover with: the MediaRemote
framework every now-playing tool used to call was restricted to entitled
applications in macOS 15.4. The trade is a fair one, since Spotify's dictionary
hands back a `spotify:track:` ID, which is exactly the key the word-by-word
database is stored under — so the best part of the program survives the crossing
intact. Neither app is ever launched by this program; one that is not already
running simply is not listed.

**Windows** is not supported yet. The shape of it is clear — the
`GlobalSystemMediaTransportControlsSessionManager` API carries title, artist,
album, status and position — but it would arrive with no Spotify track ID, and so
no word-by-word lyrics, and nobody here can test it. There is a placeholder
backend that says so rather than failing obscurely.

## Development

```bash
cargo test                      # 135 tests, no terminal or player needed
cargo clippy --all-targets -- -D warnings
cargo run --example pump_dump -- 15   # dump the live player event stream
```

The pure parts — the parser, timeline, clock, config layering, layout and match
scoring — sit behind a library target so they can be tested without a bus or a
TTY. For the parts that cannot be pure, `src/player/fake.rs` replays a scripted
event timeline, which lets the sync engine be driven at arbitrary "times"
without any sleeping. The macOS backend is split the same way: everything with a
decision in it is in `parse_probe`, so the only untested line is the one that
spawns `osascript`, which no test may do — it would prompt for Automation access
on a developer's machine and fail outright in CI.

## Limitations

* Word timings are still not universal. Three of the four sources carry them and
  between them they cover most of what you are likely to play, but an obscure
  track can still come back line-level from LRCLIB, with no moving highlight.
  `lyrics status` says which source you got and whether it carried real word
  timings.
* Two of the four sources are one person's server each. They are configurable
  and self-hostable for exactly that reason, and the lookup steps over one that
  is down rather than failing — but a permanent disappearance would cost real
  coverage.
* Because AMLL is keyed by Spotify track ID, that one source only applies when
  the player reports one. The other three match on artist and title, so they
  work anywhere.
* Anything held by none of the four will not be found, and pressing `r` will not
  conjure it into existence.
* On Linux, only MPRIS players are visible, so anything that does not expose an
  MPRIS interface goes unnoticed. On macOS it is narrower still: Spotify and
  Apple Music, and nothing else. See [Platforms](#platforms).
* macOS needs Automation permission for the player you use. Refuse the prompt and
  the program says so and points at the setting, rather than reporting that
  nothing is playing.
* Windows is not supported yet.

## Credit

The idea and the big block-letter look come from
[tacos-terminal-lyrics](https://github.com/tacoproz1/tacos-terminal-lyrics) by
tacoproz1. This is a separate program written from scratch in Rust rather than a
fork of that one, and it takes a different approach, with no music library to
scan and no pre-processing pass.

Lyrics come from [LRCLIB](https://lrclib.net), the
[AMLL TTML database](https://github.com/amll-dev/amll-ttml-db) (CC0),
[LyricsPlus](https://github.com/ibratabian17/lyricsplus) and
[lrcmux](https://github.com/f1nniboy/lrcmux) (MIT). None of them is affiliated
with this project, and the last two are run by individuals out of their own
pockets — the LyricsPlus README says as much about its own hosting. If you lean
on them, self-host: `lyricsplus_url` and `lrcmux_url` are there for it.

## License

MIT
