<div align="center">

# terminal-lyrics

**Giant block lyrics in your terminal, synced to whatever is playing.**

[![crates.io](https://img.shields.io/crates/v/terminal-lyrics?style=flat-square&color=B7410E&logo=rust&logoColor=white)](https://crates.io/crates/terminal-lyrics)
[![Latest release](https://img.shields.io/github/v/release/mgtaco/terminal-lyrics?style=flat-square&label=release&color=2EA44F&logo=github&logoColor=white)](https://github.com/mgtaco/terminal-lyrics/releases/latest)
[![platform: Linux | macOS | Windows](docs/badges/platform.svg)](#platforms)
[![Six lyrics sources](https://img.shields.io/badge/lyrics-6_synced_sources-0EA5E9?style=flat-square)](#how-it-works)
[![MIT licence](https://img.shields.io/github/license/mgtaco/terminal-lyrics?style=flat-square&color=8A2BE2)](LICENSE)

![Word-timed lyrics appearing one word at a time, a long word building up syllable by syllable](docs/demo.gif)

<sub>Rendered by the program itself. The demo words were written for it, so what
you see is the display, not anyone's lyrics.</sub>

</div>

Run one command, play something, and the lyrics appear. There is no music
library to scan, no pre-processing pass to sit through, and no Python
environment to build first.

It follows the player over MPRIS on Linux — AppleScript on macOS, the system
media controls on Windows — and pulls synced lyrics from six sources, drawn one
word at a time wherever the timings allow it.

## Install

The binary is self-contained: it links its TLS in statically and talks to your
player through whatever the platform already provides, so nothing needs
installing alongside it — the only shared libraries it wants are the C runtime,
which is already on your system. Any one of these is the whole install.

**With a Rust toolchain** (1.88 or newer):

```bash
cargo install terminal-lyrics
```

**Without one**, take the archive for your machine from the [latest
release](https://github.com/mgtaco/terminal-lyrics/releases/latest):

| | System | Archive |
| :-: | --- | --- |
| <img src="docs/icons/linux.svg" width="15" alt=""> | Linux, any distribution | `lyrics-*-x86_64-unknown-linux-musl.tar.gz` |
| <img src="docs/icons/linux.svg" width="15" alt=""> | Linux, dynamically linked | `lyrics-*-x86_64-unknown-linux-gnu.tar.gz` |
| <img src="docs/icons/apple.svg" width="15" alt=""> | macOS, Apple silicon | `lyrics-*-aarch64-apple-darwin.tar.gz` |
| <img src="docs/icons/apple.svg" width="15" alt=""> | macOS, Intel | `lyrics-*-x86_64-apple-darwin.tar.gz` |
| <img src="docs/icons/windows.svg" width="15" alt=""> | Windows | `lyrics-*-x86_64-pc-windows-msvc.tar.gz` |

```bash
tar -xzf lyrics-*.tar.gz
mkdir -p ~/.local/bin && install -m755 lyrics-*/lyrics ~/.local/bin/lyrics
```

The musl build is the one to take if you are unsure: it is statically linked, so
it does not care how old your distribution's glibc is. GitHub shows a SHA-256
next to each archive on the release page if you want to check a download.

**On Arch**, `packaging/aur-bin/` is a `PKGBUILD` that installs the release
binary through pacman, so it uninstalls cleanly like anything else:

```bash
cd packaging/aur-bin && makepkg -si
```

It is not on the AUR yet — account registration there is paused while Arch deals
with a wave of automated sign-ups — but nothing about it needs the AUR, and this
is the same `PKGBUILD` that will be published when it reopens.

**Or from a checkout**:

```bash
cargo build --release
mkdir -p ~/.local/bin && install -m755 target/release/lyrics ~/.local/bin/lyrics
```

(`install -D` would make the directory for you, but only on Linux: the `-D` that
macOS ships is a different flag entirely.)

On macOS the first run raises the standard Automation prompt, because reading
what Spotify is playing means sending it an Apple event. Allow it once and it
stops asking. If you refuse and change your mind, the switch is under System
Settings → Privacy & Security → Automation.

## Use

```bash
lyrics
```

It finds your player, reads what is playing, downloads synced lyrics and draws
them in block letters. Where the lyrics carry real per-word timings they appear
**one word at a time**, each word arriving as it is sung, while line-level
lyrics are shown a phrase at a time.

| key | |
| --- | --- |
| `q` `Esc` | quit |
| `space` | play/pause the player |
| `,` `.` | shift lyrics 100 ms earlier / later, saved for this song |
| `<` `>` | the same in whole seconds, for a song that is badly out |
| `0` | forget this song's shift and go back to the default |
| `f` | cycle font: block → compact → mini |
| `w` | one word at a time, or whole lines |
| `v` | both voices at once, or one |
| `s` | cycle the highlight: never → auto → always |
| `r` | forget the cached lyrics and look them up again |

The shift belongs to the song, not to the session. A file that plays 300 ms late
is fixed once with `,` — or, when a set of lyrics is timed against a different
master and is out by two or three whole seconds, with `<` and `>` first and `,`
and `.` to finish — and the correction is written to disk against that track
and reapplied every time it comes on again — while the next song starts from
`offset_ms` in the config, which is what that setting now means: where a song
starts before anyone has nudged it. `0` forgets a song's shift and puts it back
there. `lyrics paths` prints the file and how many songs are in it, and `lyrics
status` shows whether the song playing has a saved shift.

The flags worth knowing:

| | |
| --- | --- |
| `--player spotify` | pin one player when several are running |
| `--font compact` | smaller letters: `block`, `compact` or `mini` |
| `--whole-lines` | always show the full phrase, never word by word |
| `--sweep` | highlight the sung part of whatever is on screen |
| `--single-voice` | one line at a time, even when two people are singing |
| `--lrc-dir ~/lyrics` | prefer your own `Artist - Title.lrc` files |
| `--no-network` | use only those files and the cache |
| `--offset-ms -250` | starting point for every song you have not tuned by hand |
| `--color-source pywal` | take one accent colour from a palette |

## Without the TUI

```bash
lyrics status                 # player, track, position, and which source matched
lyrics fetch --artist "Radiohead" --title "Creep" --duration 238
lyrics fetch --artist "Daft Punk" --title "Get Lucky" --json   # what DMS caches
lyrics paths                  # where the config, cache and saved offsets live
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

Per-song offsets are not settings and do not live in the config file; they are
kept as JSON in the data directory — `~/.local/share/terminal-lyrics/offsets.json`,
or `~/Library/Application Support/terminal-lyrics/offsets.json` on macOS. Only
songs you have actually nudged appear in it, so it stays small enough to read
and edit by hand, and deleting it just returns every song to the default.

Colour is the one setting worth explaining here, because the default is to have
no opinion. The lyrics are drawn in your terminal's own palette entries, so they
already match whatever scheme it is set to, and there is deliberately no list of
built-in themes to pick from — that would trade the matching away for a menu.
What there is instead is a source for one accent:

```bash
lyrics --color-source pywal            # follow ~/.cache/wal/colors.json
lyrics --color-source 'fixed:#8a2be2'  # one literal colour
lyrics --color-source file:PATH        # any palette in pywal's JSON shape
```

Only the lyric text is repainted; the sweep highlight and the status lines stay
palette colours, so the display still follows the terminal everywhere else. A
palette that cannot be read falls back to the terminal rather than refusing to
start, and `lyrics status` says when that has happened.

## How it works

### Choosing a player

Without `--player`, whichever player is actually playing wins, followed by one
with a track loaded and then anything else, with the name as a stable tiebreak.
Simply taking the first name alphabetically turns out to be wrong in practice,
because browsers register idle MPRIS instances such as `chromium.instance26065`
that sort ahead of `spotify` while reporting no track at all. That ranking is
platform-neutral and shared by both backends. If the wrong one is being picked,
`lyrics status` lists what each of them is doing.

### Position

The player is asked once and the position is interpolated from a monotonic clock
from then on, re-anchoring whenever the player says something. A 1 Hz position
read covers the players that seek without announcing it, Spotify among them. On
Linux that read costs no subprocess at all; on macOS it is one short-lived
`osascript`, which is also what makes a scrub show up within a second there
despite AppleScript having no seek notification to offer.

### Sources

The lookup is DankMaterialShell's, ported line for line from its Go library
([dankgo/lyrics](https://github.com/AvengeMedia/dankgo)), so that `lyrics` and
the DMS lyrics widget pick the same lyrics for the same song and stay in step
when both are on screen. They also share one cache: when
`~/.cache/DankMaterialShell` exists, lyrics are cached in its `lyrics/`
directory, in the same files and the same format, so whichever of the two looks
a song up first answers for the other.

Ahead of the network sit your own `--lrc-dir` files and, for a local file the
player hands over as a `file://` URL, a lyrics file next to it: `song.lyricsfile.yaml`,
`song.ttml`, `song.lrc` or `song.LRC`, in that order. Then six providers:

| source | timing | |
| --- | :-: | --- |
| [BetterLyrics](https://better-lyrics.boidu.dev) | syllable | Apple Music's TTML, matched on title, artist, album and length |
| [Unison](https://unison.betterlyrics.org) | syllable | BetterLyrics' community-submitted lyrics, TTML or enhanced LRC |
| [LyricsPlus](https://github.com/ibratabian17/lyricsplus) | syllable | JSON with per-syllable timing and backing vocals marked word by word |
| KuGou | word | its KRC format, decrypted and inflated; translations baked into the lyrics are dropped |
| [LRCLIB](https://lrclib.net) | line | millions of tracks, line-level nearly always, now and then a word-timed Lyricsfile |
| YouTube Music | line | the lyrics YouTube Music shows, found through its own search |

```toml
providers = ["betterlyrics", "unison", "lyricsplus", "kugou", "lrclib", "youtubemusic"]
```

Dropping a name turns that provider off, and the order is their priority. They
are all asked at once, and the answer is chosen in two steps: better timing
first (word over line over plain), then priority among answers that are equally
good. So a fast line-level answer from LRCLIB is held while a word-timed
provider is still out, and a fast word-timed answer from low in the list is held
while one ahead of it could still match it. Once priority alone has a winner,
the wait for better timing is two seconds at most, and the whole lookup twelve.
A provider that is down is stepped over; the lookup only fails when every
provider failed and none of them managed to say "not here". Lyrics with no
timing at all are not shown.

Each provider keeps its own cache entry for each track, keyed on the artist,
title and album folded to lower case and the length in whole seconds. A hit is
kept until it is ninety days old; a miss is asked again after fourteen days, and
a line-level hit from a provider that usually has word timings is asked again
once after fourteen days in case it has them by then. `r` forgets every
provider's entry for the song playing and asks again.

### Matching LRCLIB

An exact `/api/get` on artist, title, album and length comes first. If that
answers with plain lyrics or nothing, a title search follows, and the best record
for the same title and artist within eight seconds of the track's length wins:
word timing over line timing over plain, then the closest length.

### Word timing

What appears on screen follows the lyrics rather than a preference. A source
carrying real per-word timestamps — five of the six usually do — is shown one word
at a time with each word appearing on its own timestamp, whereas a line-level
source has nothing to split on and so is shown whole. Because that decision is
made per line rather than per file, a file that carries tags on only some of its
lines still shows the rest in full. Where a long word is timed in pieces, the
pieces build the word up in place instead of flashing a syllable on its own.

### Highlight

The highlight is a separate thing and is off unless you ask for it. `--sweep`
lights up the sung part of whatever is on screen, moving across the current word
in word-by-word mode and across the phrase otherwise. For line-level lyrics it
has to be interpolated from character counts between two real timestamps, which
makes it an estimate, and `lyrics status` labels it as one. It remains a display
effect throughout: nothing invented is ever written to disk.

`auto` is the middle setting and it asks what the highlight would actually be
telling you. Its job is to say which word of the line is being sung, so it wants
real per-word timestamps to place it — and it wants the rest of the line there to
place it against. One word at a time already answers the question by itself, so
`auto` stays out of the way whenever word-by-word is on. `--sweep` overrides
that: it sweeps the syllables of the word on screen.

### Two voices

Where two people are singing at once, both are on screen. A backing vocal — the
`(ooh ooh)` behind the line, which the syllable-timed sources mark as its own
voice — sits above the line in grey and a size smaller, because it is texture
rather than the thing being read; it is drawn whole and never swept, and the line
underneath keeps the highlight. The other half of a duet is drawn full size and
lit, since it is a voice and not an echo, and while one is up the line beneath it
is shown whole rather than a word at a time.

Overlap alone is not enough to call something a duet. Across 39 files from the
AMLL database the median overlap between one line and the next is a quarter of a
second, and three quarters of them are under a second: that is one phrase's tail
running into the next one's head, not two people singing, and stacking it would
flicker a second line up a couple of times a song. So a duet has to overlap for a
full second and for half of the shorter phrase. A backing vocal needs no such
test — the source says outright that it is a second voice.

### Drawing

ratatui diffs the screen buffer, so a frame that has not changed emits no bytes
at all. Lines wrap at word boundaries and the font steps down a size before
anything would be clipped, so nothing is ever truncated. Where two voices will
not fit, both step down together first, and only if even the smallest font cannot
hold them is the second voice dropped — the line being read then goes back to the
size it would have had on its own, rather than staying shrunk for something that
is no longer there.

## Platforms

The program is the same everywhere; only the way it finds your player differs,
and that lives behind one seam in `src/player/`. Exactly one backend is compiled
into a given binary, so a Linux package never carries the macOS code or the other
way round.

### <img src="docs/icons/linux.svg" width="17" alt=""> Linux

MPRIS over D-Bus, directly, with no subprocess anywhere: property changes and
`Seeked` arrive as signals. Anything exposing an MPRIS interface works, which is
most things.

### <img src="docs/icons/apple.svg" width="17" alt=""> macOS

Spotify and Apple Music through their scripting dictionaries, polled once a
second. It knows those two by name rather than discovering players generally,
because there is nothing left to discover with: the MediaRemote framework every
now-playing tool used to call was restricted to entitled applications in macOS
15.4. Every provider matches on artist and title, so nothing about the lyrics
is lost in the crossing. Neither app
is ever launched by this program; one that is not already running simply is not
listed.

### <img src="docs/icons/windows.svg" width="17" alt=""> Windows

The System Media Transport Controls — the API behind the media flyout in the
volume panel — polled like macOS. Like MPRIS and unlike AppleScript it discovers
players rather than knowing them by name, so anything that registers a session
can be followed. Every provider matches on artist and title, so word-by-word
lyrics work there as well.

## Limitations

* Word timings are still not universal. Five of the six sources usually carry
  them, but an obscure track can still come back line-level from LRCLIB or
  YouTube Music, with no moving highlight. `lyrics status` says which source you
  got and whether it carried real word timings.
* Most of the sources are community-run services, and some are unofficial APIs
  that can change without notice. The lookup steps over one that is down rather
  than failing, but a permanent disappearance would cost real coverage.
* The lookup follows DankMaterialShell's, so a better match DMS would not pick is
  not picked here either. That is the price of the two agreeing.
* Anything held by none of the six will not be found, and pressing `r` will not
  conjure it into existence.
* On Linux, only MPRIS players are visible, so anything that does not expose an
  MPRIS interface goes unnoticed. On Windows the equivalent is registering with
  the media controls. On macOS it is narrower than either: Spotify and Apple
  Music, and nothing else. See [Platforms](#platforms).
* macOS needs Automation permission for the player you use. Refuse the prompt and
  the program says so and points at the setting, rather than reporting that
  nothing is playing.

## Credit

The idea and the big block-letter look come from
[tacos-terminal-lyrics](https://github.com/tacoproz1/tacos-terminal-lyrics) by
tacoproz1. This is a separate program written from scratch in Rust rather than a
fork of that one, and it takes a different approach, with no music library to
scan and no pre-processing pass.

The lyrics lookup is a port of DankMaterialShell's
([dankgo/lyrics](https://github.com/AvengeMedia/dankgo), © Avenge Media LLC, MIT,
see [licenses/dankgo-MIT.txt](licenses/dankgo-MIT.txt)), so that the two agree. Lyrics come from BetterLyrics and Unison,
[LyricsPlus](https://github.com/ibratabian17/lyricsplus), KuGou,
[LRCLIB](https://lrclib.net) and YouTube Music. None of them is affiliated with
this project.

The platform marks in `docs/icons/` are based on
[Simple Icons](https://simpleicons.org) (CC0).

## License

MIT
