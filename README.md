<div align="center">

# terminal-lyrics

**Giant block lyrics in your terminal, synced to whatever is playing.**

[![crates.io](https://img.shields.io/crates/v/terminal-lyrics?style=flat-square&color=B7410E&logo=rust&logoColor=white)](https://crates.io/crates/terminal-lyrics)
[![Latest release](https://img.shields.io/github/v/release/mgtaco/terminal-lyrics?style=flat-square&label=release&color=2EA44F&logo=github&logoColor=white)](https://github.com/mgtaco/terminal-lyrics/releases/latest)
[![platform: Linux | macOS | Windows](docs/badges/platform.svg)](#platforms)
[![Seven lyrics sources](https://img.shields.io/badge/lyrics-7_synced_sources-0EA5E9?style=flat-square)](#how-it-works)
[![MIT licence](https://img.shields.io/github/license/mgtaco/terminal-lyrics?style=flat-square&color=8A2BE2)](LICENSE)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/demo-dark.gif">
  <img alt="Word-timed lyrics appearing one word at a time, a long word building up syllable by syllable, first in capitals and then in lowercase" src="docs/demo-light.gif">
</picture>

<sub>Rendered by the program itself.</sub>

</div>

Run `lyrics`, play something, and the lyrics appear, one word at a time
wherever the timings allow it. It follows your music player and pulls synced
lyrics from seven sources. There is no library to scan and nothing to
pre-process.

## Install

**Linux or macOS**

```bash
curl -fsSL https://raw.githubusercontent.com/mgtaco/terminal-lyrics/main/install.sh | sh
```

**With a Rust toolchain** (1.88 or newer)

```bash
cargo install terminal-lyrics
```

**Windows**: download `lyrics-*-x86_64-pc-windows-msvc.tar.gz` from the
[latest release](https://github.com/mgtaco/terminal-lyrics/releases/latest),
unpack it with `tar -xzf` (built into Windows 10 and later) and put
`lyrics.exe` somewhere on your `PATH`.

Then run `lyrics` and play something. On macOS the first run asks to let your
terminal control Spotify or Music, which is how it reads what is playing; allow
it once and it stops asking. If you refused, the switch is under System
Settings → Privacy & Security → Automation.

<details>
<summary><b>About the install script, and other ways to install</b></summary>

[`install.sh`](install.sh) picks the archive for your machine from the latest
release, checks it against the SHA-256 GitHub computed when it was uploaded,
and puts `lyrics` in `~/.local/bin`, saying so if that is not on your `PATH`
yet. It does not go through cargo or a package manager: run it again to update,
and delete `~/.local/bin/lyrics` to uninstall. Set `LYRICS_VERSION=0.3.0` for a
particular release or `LYRICS_INSTALL_DIR` for another directory, on the `sh`
side of the pipe. To read it before it runs, download it, look it over, and
then `sh install.sh`:

```bash
curl -fsSLO https://raw.githubusercontent.com/mgtaco/terminal-lyrics/main/install.sh
```

Whichever way it gets there, the binary is self-contained: it links its TLS in
statically and talks to your player through whatever the platform already
provides, so nothing needs installing alongside it.

**By hand**, take the archive for your machine from the
[latest release](https://github.com/mgtaco/terminal-lyrics/releases/latest):

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

On Linux the musl build is the one to take if you are unsure: it is statically
linked, so it does not care how old your distribution's glibc is. GitHub shows
a SHA-256 next to each archive on the release page if you want to check a
download.

**On Arch**, `packaging/aur-bin/` is a `PKGBUILD` that installs the release
binary through pacman, so it uninstalls cleanly like anything else:

```bash
cd packaging/aur-bin && makepkg -si
```

It is not on the AUR yet, because account registration there is paused, but
this is the same `PKGBUILD` that will be published when it reopens.

**From a checkout**:

```bash
cargo build --release
mkdir -p ~/.local/bin && install -m755 target/release/lyrics ~/.local/bin/lyrics
```

(`install -D` would make the directory for you, but only on Linux: the `-D` that
macOS ships is a different flag entirely.)

</details>

## Use

```bash
lyrics
```

| key | |
| --- | --- |
| `q` `Esc` | quit |
| `space` | play/pause the player |
| `,` `.` | shift lyrics 100 ms earlier / later, saved for this song |
| `<` `>` | the same in whole seconds, for a song that is badly out |
| `0` | forget this song's shift and go back to the default |
| `f` | cycle font: block → compact → mini |
| `c` | switch between capitals and lowercase |
| `w` | one word at a time, or whole lines |
| `s` | cycle the highlight: never → auto → always |
| `v` | [cava](https://github.com/karlstav/cava)'s spectrum bars under the lyrics, on or off |
| `h` | hide or show the lyrics source in the bottom-left corner |
| `r` | forget the cached lyrics and look them up again |

What `f`, `c`, `w`, `s`, `v` and `h` choose is saved to your config file, so the
next run starts the way you left it. A shift belongs to the song: fix one that
runs late once and it stays fixed every time it comes on again. Closing the
player does not close the lyrics; they pick up the next player that starts.

The flags worth knowing (`lyrics --help` lists them all):

| | |
| --- | --- |
| `--player spotify` | pin one player when several are running |
| `--font compact` | smaller letters: `block`, `compact` or `mini` |
| `--case lower` | lowercase letters instead of capitals |
| `--whole-lines` | always show the full phrase, never word by word |
| `--sweep` | highlight the sung part of whatever is on screen |
| `--single-voice` | one line at a time, even when two people are singing |
| `--lrc-dir ~/lyrics` | prefer your own `Artist - Title.lrc` files |
| `--no-network` | use only those files and the cache |
| `--offset-ms -250` | starting point for every song you have not tuned by hand |
| `--color-source pywal` | take one accent colour from a palette |
| `--dim-color '#9a9cae'` | a fixed colour for background vocals, instead of faint |
| `--visualizer` | start with cava's spectrum bars on |
| `--hide-source` | leave the bottom-left corner empty instead of naming the source |

<details>
<summary><b>Per-song sync</b></summary>

A file that plays 300 ms late is fixed once with `,`. When a set of lyrics is
timed against a different master and is out by two or three whole seconds, use
`<` and `>` first and `,` and `.` to finish. The correction is written to disk
against that track and reapplied every time it comes on again, while the next
song starts from `offset_ms` in the config: where a song starts before anyone
has nudged it. `0` forgets a song's shift and puts it back there.

The shifts are kept as JSON in the data directory,
`~/.local/share/terminal-lyrics/offsets.json`, or
`~/Library/Application Support/terminal-lyrics/offsets.json` on macOS. Only
songs you have actually nudged appear in it, so it stays small enough to read
and edit by hand, and deleting it returns every song to the default.
`lyrics paths` prints the file and how many songs are in it, and
`lyrics status` shows whether the song playing has a saved shift.

</details>

<details>
<summary><b>Visualizer</b></summary>

With [cava](https://github.com/karlstav/cava) installed, `v` (or
`--visualizer`, or `visualizer = true` in the config) puts its spectrum under
the lyrics: the bars stand on the status line and reach halfway up the space
between them, in the same faint colour as the status line. Your own
`~/.config/cava/config` is read for its input settings, so if cava already
captures one player's PipeWire node, so do these bars; only the bar count and
output are replaced. Without cava nothing is drawn.

</details>

### Without the TUI

```bash
lyrics status                 # player, track, position, and which source matched
lyrics fetch --artist "Radiohead" --title "Creep" --duration 238
lyrics fetch --artist "Kanye West" --title "Flashing Lights" \
             --spotify-id 5TRPicyLGbAF2LGBFbHGvO   # word-timed, from AMLL
lyrics fetch --artist "Daft Punk" --title "Get Lucky" --json   # the full answer, as JSON
lyrics paths                  # where the config, cache and saved offsets live
```

When something is not working, start with `lyrics status`. It prints every
player and what each one is doing, the track it settled on, the length the
player reported, the cache key, whether the lyrics carry real word timings and
where they came from.

## Configuration

Settings live in `~/.config/terminal-lyrics/config.toml`, or
`~/Library/Application Support/terminal-lyrics/config.toml` on macOS;
`lyrics paths` prints the one this build actually reads.
[`config.example.toml`](config.example.toml) lists every key. Flags override
the file and the file overrides the defaults. The `f`, `c`, `w`, `s`, `v` and `h`
keys write their choice into this file, creating it if need be; only that one
line changes, and your comments and layout are kept.

<details>
<summary><b>Colours</b></summary>

By default the lyrics are drawn in your terminal's own palette entries, so they
already match whatever scheme it is set to, and there is deliberately no list of
built-in themes to pick from: that would trade the matching away for a menu.
What there is instead is a source for one accent:

```bash
lyrics --color-source pywal            # follow ~/.cache/wal/colors.json
lyrics --color-source 'fixed:#8a2be2'  # one literal colour
lyrics --color-source file:PATH        # any palette in pywal's JSON shape
```

Only the lyric text is repainted, background vocals and status lines included,
since they are the same colour drawn faint; the sweep highlight stays a palette
colour, so the display still follows the terminal everywhere else. A palette
that cannot be read falls back to the terminal rather than refusing to start,
and `lyrics status` says when that has happened.

Faint is the terminal's own attribute rather than a grey picked here, so it
holds up on translucent and blurred backgrounds where the palette's dark grey
all but disappears. If your terminal draws it too weak, too strong or not at
all, `dim_color = "#rrggbb"` (or `--dim-color`) names the colour outright.

</details>

## Platforms

| | Follows | |
| :-: | --- | --- |
| <img src="docs/icons/linux.svg" width="15" alt=""> | any MPRIS player | over D-Bus, with no subprocess anywhere |
| <img src="docs/icons/apple.svg" width="15" alt=""> | Spotify and Apple Music | through their scripting dictionaries, once a second |
| <img src="docs/icons/windows.svg" width="15" alt=""> | anything in the system media controls | the media flyout's API, once a second |

<details>
<summary><b>Details per platform</b></summary>

The program is the same everywhere; only the way it finds your player differs,
and that lives behind one seam in `src/player/`. Exactly one backend is compiled
into a given binary, so a Linux package never carries the macOS code or the
other way round.

**Linux**: MPRIS over D-Bus, directly, with no subprocess anywhere: property
changes and `Seeked` arrive as signals. Anything exposing an MPRIS interface
works, which is most things.

**macOS**: Spotify and Apple Music through their scripting dictionaries, polled
once a second. It knows those two by name rather than discovering players
generally, because there is nothing left to discover with: the MediaRemote
framework every now-playing tool used to call was restricted to entitled
applications in macOS 15.4. Spotify still hands over its track id, so AMLL
works there as on Linux. Neither app is ever launched
by this program; one that is not already running simply is not listed.

**Windows**: the System Media Transport Controls, the API behind the media
flyout in the volume panel, polled like macOS. Like MPRIS and unlike
AppleScript it discovers players rather than knowing them by name, so anything
that registers a session can be followed. What it does not carry is a Spotify
track id, so AMLL is stepped over; every other provider matches on artist and
title, so word-by-word lyrics work there as well.

</details>

## How it works

### Choosing a player

Without `--player`, whichever player is actually playing wins, followed by one
with a track loaded and then anything else, with the name as a stable tiebreak.
Simply taking the first name alphabetically turns out to be wrong in practice,
because browsers register idle MPRIS instances such as `chromium.instance26065`
that sort ahead of `spotify` while reporting no track at all. That ranking is
platform-neutral and shared by every backend. If the wrong one is being picked,
`lyrics status` lists what each of them is doing.

### Position

The player is asked once and the position is interpolated from a monotonic clock
from then on, re-anchoring whenever the player says something. A 1 Hz position
read covers the players that seek without announcing it, Spotify among them. On
Linux that read costs no subprocess at all; on macOS it is one short-lived
`osascript`, which is also what makes a scrub show up within a second there
despite AppleScript having no seek notification to offer.

### Sources

Ahead of the network sit your own `--lrc-dir` files and, for a local file the
player hands over as a `file://` URL, a lyrics file next to it:
`song.lyricsfile.yaml`, `song.ttml`, `song.lrc` or `song.LRC`, in that order.
Then seven providers:

| source | timing | |
| --- | :-: | --- |
| [AMLL TTML DB](https://github.com/amll-dev/amll-ttml-db) | syllable | timed by hand, so the most accurate when it has the song, but only a few thousand songs. Keyed by Spotify track id, so only asked when the player is Spotify |
| [BetterLyrics](https://better-lyrics.boidu.dev) | syllable | Apple Music's TTML, matched on title, artist, album and length |
| [Unison](https://unison.betterlyrics.org) | syllable | BetterLyrics' community-submitted lyrics, TTML or enhanced LRC |
| [LyricsPlus](https://github.com/ibratabian17/lyricsplus) | syllable | JSON with per-syllable timing and backing vocals marked word by word |
| [LRCLIB](https://lrclib.net) | line | millions of tracks, line-level nearly always, now and then a word-timed Lyricsfile |
| YouTube Music | line | the lyrics YouTube Music shows, found through its own search |
| KuGou | word | its KRC format, decrypted and inflated; translations baked into the lyrics are dropped. Its timings are often off, so it comes last and wins only when nothing else has the song word-timed |

```toml
providers = ["amll", "betterlyrics", "unison", "lyricsplus", "lrclib", "youtubemusic", "kugou"]
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
carrying real per-word timestamps (six of the seven usually do) is shown one
word at a time, each word appearing on its own timestamp, whereas a line-level
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
real per-word timestamps to place it, and it wants the rest of the line there
to place it against. One word at a time already answers the question by itself,
so `auto` stays out of the way whenever word-by-word is on. `--sweep` overrides
that: it sweeps the syllables of the word on screen.

### Two voices

Where two people are singing at once, both are on screen. A backing vocal (the
`(ooh ooh)` behind the line, which the syllable-timed sources mark as its own
voice) sits above the line, faint and a size smaller, because it is texture
rather than the thing being read. In word-by-word mode it steps through its own
words alongside the line's when the source times them, but it is never swept:
the line underneath keeps the highlight. The other half of a duet is drawn full
size and lit, since it is a voice and not an echo, and is treated exactly like
the line: in word-by-word mode both voices show the word each is singing, side
by side in time, and the sweep runs across both. Only when the partner has no
word timings of its own are the two shown as whole phrases.

Overlap alone is not enough to call something a duet. Across 39 files from the
AMLL database the median overlap between one line and the next is a quarter of a
second, and three quarters of them are under a second: that is one phrase's tail
running into the next one's head, not two people singing, and stacking it would
flicker a second line up a couple of times a song. So a duet has to overlap for a
full second and for half of the shorter phrase. A backing vocal needs no such
test: the source says outright that it is a second voice.

### Drawing

Letters are pixel art drawn with half blocks (`▀ ▄ █`), so one pixel is a column
wide and half a row tall, which is square on most terminals. Capitals,
lowercase, digits and punctuation are drawn by hand; an accented letter is its
base letter with the mark stamped on, which covers every Latin-script language,
Vietnamese's stacked accents included, Cyrillic for Russian, Ukrainian,
Belarusian, Bulgarian, Serbian and Macedonian, and Greek, which drops its
accents in capitals as Greek lettering does. Chinese, Japanese and Korean are far too many characters to draw by
hand, so they come from a 10-pixel CJK font, exactly the capitals' height; a
line without spaces wraps between any two characters, though never so as to
start a line on a small kana. A line takes the capitals' five rows,
plus a row or two only when something in it needs them (an accent on a capital,
a descender), and the capitals stay where they were, so a word with a `g` in it
does not bob. A line mostly in a script with no art (Arabic, Hebrew, Thai) is
shown as plain text, and a stray symbol the font has no art for is shown as
itself rather than dropped.

ratatui diffs the screen buffer, so a frame that has not changed emits no bytes
at all. Lines wrap at word boundaries and the font steps down a size before
anything would be clipped, so nothing is ever truncated. Where two voices will
not fit, both step down together first, and only if even the smallest font cannot
hold them is the second voice dropped. The line being read then goes back to the
size it would have had on its own, rather than staying shrunk for something that
is no longer there.

## Limitations

* Word timings are not universal. Six of the seven sources usually carry them,
  but an obscure track can still come back line-level from LRCLIB or YouTube
  Music. `lyrics status` says which source you got.
* Most of the sources are community-run, and some are unofficial APIs that can
  change without notice. One that is down is stepped over, but one that
  disappears for good would cost real coverage.
* Anything held by none of the seven will not be found, and pressing `r` will not
  conjure it into existence.
* Only players the platform exposes are visible: MPRIS on Linux, the media
  controls on Windows, and on macOS just Spotify and Apple Music. See
  [Platforms](#platforms).
* macOS needs Automation permission for the player you use. Refuse the prompt and
  the program says so and points at the setting, rather than reporting that
  nothing is playing.

## Credit

The idea and the big block-letter look come from
[tacos-terminal-lyrics](https://github.com/tacoproz1/tacos-terminal-lyrics) by
tacoproz1. This is a separate program written from scratch in Rust rather than a
fork of that one, and it takes a different approach, with no music library to
scan and no pre-processing pass.

Lyrics come from the
[AMLL TTML database](https://github.com/amll-dev/amll-ttml-db) (CC0),
BetterLyrics and Unison,
[LyricsPlus](https://github.com/ibratabian17/lyricsplus), KuGou,
[LRCLIB](https://lrclib.net) and YouTube Music. None of them is affiliated with
this project. Third-party licences are in [licenses/](licenses/).

The Chinese, Japanese and Korean characters are from
[Fusion Pixel Font](https://github.com/TakWolf/fusion-pixel-font) by TakWolf
(SIL Open Font License 1.1), converted to the program's own format. The
Cyrillic letters were drawn by [deepsytxt](https://github.com/deepsytxt).

The platform marks in `docs/icons/` are based on
[Simple Icons](https://simpleicons.org) (CC0).

## License

MIT
