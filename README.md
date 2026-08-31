<div align="center">

# terminal-lyrics

**Giant block lyrics in your terminal, synced to whatever is playing.**

[![crates.io](https://img.shields.io/crates/v/terminal-lyrics?style=flat-square&color=B7410E&logo=rust&logoColor=white)](https://crates.io/crates/terminal-lyrics)
[![Latest release](https://img.shields.io/github/v/release/mgtaco/terminal-lyrics?style=flat-square&label=release&color=2EA44F&logo=github&logoColor=white)](https://github.com/mgtaco/terminal-lyrics/releases/latest)
[![platform: Linux | macOS | Windows](docs/badges/platform.svg)](#platforms)
[![Four lyrics sources](https://img.shields.io/badge/lyrics-4_synced_sources-0EA5E9?style=flat-square)](#how-it-works)
[![MIT licence](https://img.shields.io/github/license/mgtaco/terminal-lyrics?style=flat-square&color=8A2BE2)](LICENSE)

![Word-timed lyrics appearing one word at a time, a long word building up syllable by syllable](docs/demo.gif)

<sub>Rendered by the program itself. The demo words were written for it, so what
you see is the display, not anyone's lyrics.</sub>

</div>

Run one command, play something, and the lyrics appear. There is no music
library to scan, no pre-processing pass to sit through, and no Python
environment to build first.

It follows the player over MPRIS on Linux — AppleScript on macOS, the system
media controls on Windows — and pulls synced lyrics from four sources, drawn one
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
| `--lrcmux-sources all` | let lrcmux use every upstream, KuGou included |
| `--color-source pywal` | take one accent colour from a palette |

## Without the TUI

```bash
lyrics status                 # player, track, position, and which source matched
lyrics fetch --artist "Radiohead" --title "Creep" --duration 238
lyrics fetch --artist "Kanye West" --title "Flashing Lights" \
             --spotify-id 5TRPicyLGbAF2LGBFbHGvO   # word-timed, via AMLL
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

`--lrc-dir` first, then the cache, then four networks in order. The order is by
what each is good at rather than by how often it answers, so a better-looking
set of lyrics is never passed over for a merely likelier one:

| source | timing | |
| --- | :-: | --- |
| [AMLL TTML DB](https://github.com/amll-dev/amll-ttml-db) | syllable | CC0 and community-maintained, keyed by the Spotify track ID the player already hands us, so it needs neither a search nor any matching guesswork. It holds about 2,400 entries, so it costs one request and misses often — but when it hits, nothing else needs asking |
| [LyricsPlus](https://github.com/ibratabian17/lyricsplus) | syllable | Apple Music's own TTML, which splits a long word into pieces the display then builds up in place. Matched on artist and title, with the length Apple records in the document used to throw out a different edit. Its `platformId=spotify:…` parameter is not used: the public instance has no Spotify credentials to resolve an ID with, and answers every such query 404 |
| [lrcmux](https://github.com/f1nniboy/lrcmux) | word | one API in front of Musixmatch richsync, KuGou, YouTube Music, Genius and LRCLIB, and the widest of the four: five upstreams means it degrades rather than dies. It says outright whether the answer it found carries word timings, and which upstream they came from |
| [LRCLIB](https://lrclib.net) | line | millions of tracks, but line-level timing only. Every one of the 923 `syncedLyrics` entries sampled was line-level, which is why it is asked last rather than first |

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

lrcmux is five sources wearing one coat, and they are not equally good, so it
has a filter of its own:

```toml
lrcmux_sources = ["!kugou"]
```

Either a list to allow or a list to exclude, never a mix — an allow-list already
excludes everything it does not name. The order is not a preference: asking
lrcmux for `musixmatch,kugou` and for `kugou,musixmatch` returns the same
answer, because it ranks its upstreams itself. `[]` lets it choose freely.

KuGou is excluded by default, and it is worth being precise about why, because
it is not the usual complaint about timing. Across 111 tracks, KuGou's words
overlapped known-good lyrics with a median Jaccard of 0.86 against Musixmatch's
0.97, and its lower quartile ran down to 0.20 — whole songs of the wrong words,
confidently timed, which is what an automatic transcription of English by a
Chinese service looks like. A wrong offset can be nudged back into place; wrong
words cannot. And since lrcmux prefers KuGou when both have an answer, nothing
short of excluding it helps. It is only *excluded*, so a track KuGou alone would
have covered now falls through to LRCLIB's line-level lyrics rather than to
nothing. `lyrics status` names the upstream that answered.

Musixmatch, the upstream that usually answers in its place, has the opposite
problem: the words are right and the whole document is sometimes timed against a
different master, out by two or three seconds from the first line to the last.
That is one shift, so `<` and `>` fix it in three keypresses and the correction
is saved against the song. Deriving that shift automatically — aligning against
LRCLIB's line timings and correcting by the offset — was tried and abandoned: at
a confidence threshold tight enough to be safe it fired on 2 of 78 tracks, and
loosening it started "correcting" songs where LRCLIB was the one that was wrong.

That order is a prediction about what each source is usually good at, and it is
not taken on trust. Only word timings end the search: a provider that answers a
line at a time is held as a fallback and the next one is asked anyway, so an
Apple document that happens to be line-level cannot step over word timings
lrcmux was holding all along. The fallback is returned once nothing better has
turned up, and the first one found wins, which keeps the order above deciding
between answers of equal quality.

One kind of answer is not held as a fallback at all but stepped over as if the
provider had said "not here": lyrics whose last line starts more than fifteen
seconds after the song has ended. Those were timed against a longer recording,
which means every line before it is shifted too — a real answer about the wrong
edit, and worse than the line-level one underneath it.

The same reasoning reaches the cache. A word-timed hit is kept indefinitely,
since nothing better is coming for it, but a line-level hit expires after a day
like a miss does — often it only means the two hobby-run providers were
unreachable for the minute that track was playing, and keeping it forever would
make a brief outage permanent for whatever you happened to be listening to.

### Matching LRCLIB

The lookup starts with an exact `/api/get` on artist, title, album and duration,
retries without the album, falls back to a scored `/api/search`, and finally
tries once more with decorations like `- Remastered 2011` and `(feat. …)`
stripped from the title. Candidates more than five seconds away from the track's
real length are rejected rather than displayed out of sync, and misses are cached
for a day so that a track LRCLIB has never heard of is not queried again on every
play.

### Word timing

What appears on screen follows the lyrics rather than a preference. A source
carrying real per-word timestamps — three of the four now do — is shown one word
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
15.4. The trade is a fair one, since Spotify's dictionary hands back a
`spotify:track:` ID, which is exactly the key the word-by-word database is stored
under — so the best part of the program survives the crossing intact. Neither app
is ever launched by this program; one that is not already running simply is not
listed.

### <img src="docs/icons/windows.svg" width="17" alt=""> Windows

The System Media Transport Controls — the API behind the media flyout in the
volume panel — polled like macOS. Like MPRIS and unlike AppleScript it discovers
players rather than knowing them by name, so anything that registers a session
can be followed. What it does not carry is a track ID, so AMLL is stepped over
there; LyricsPlus and lrcmux match on artist and title, so word-by-word lyrics
still work.

## Limitations

* Word timings are still not universal. Three of the four sources carry them and
  between them they cover most of what you are likely to play, but an obscure
  track can still come back line-level from LRCLIB, with no moving highlight.
  `lyrics status` says which source you got and whether it carried real word
  timings.
* Two of the four sources are one person's server each. They are configurable and
  self-hostable for exactly that reason, and the lookup steps over one that is
  down rather than failing — but a permanent disappearance would cost real
  coverage.
* Because AMLL is keyed by Spotify track ID, that one source only applies when
  the player reports one. The other three match on artist and title, so they work
  anywhere.
* Anything held by none of the four will not be found, and pressing `r` will not
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

Lyrics come from [LRCLIB](https://lrclib.net), the
[AMLL TTML database](https://github.com/amll-dev/amll-ttml-db) (CC0),
[LyricsPlus](https://github.com/ibratabian17/lyricsplus) and
[lrcmux](https://github.com/f1nniboy/lrcmux) (MIT). None of them is affiliated
with this project, and the last two are run by individuals out of their own
pockets — the LyricsPlus README says as much about its own hosting. If you lean
on them, self-host: `lyricsplus_url` and `lrcmux_url` are there for it.

The platform marks in `docs/icons/` are based on
[Simple Icons](https://simpleicons.org) (CC0).

## License

MIT
