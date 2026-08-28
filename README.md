<div align="center">

# terminal-lyrics

**Giant block lyrics in your terminal, in time with whatever you're playing.**

[![crates.io](https://img.shields.io/crates/v/terminal-lyrics?style=flat-square&color=B7410E&logo=rust&logoColor=white)](https://crates.io/crates/terminal-lyrics)
[![Download](https://img.shields.io/github/v/release/mgtaco/terminal-lyrics?style=flat-square&label=download&color=2EA44F&logo=github&logoColor=white)](https://github.com/mgtaco/terminal-lyrics/releases/latest)
[![Linux · macOS · Windows](https://img.shields.io/badge/Linux%20%C2%B7%20macOS%20%C2%B7%20Windows-64748B?style=flat-square)](#-platforms)
[![MIT licence](https://img.shields.io/github/license/mgtaco/terminal-lyrics?style=flat-square&color=8A2BE2)](LICENSE)

![Word-timed lyrics appearing one word at a time, a long word building up syllable by syllable](docs/demo.gif)

<sub>Rendered by the program itself. The demo words were written for it, so what
you see is the display, not anyone's lyrics.</sub>

</div>

Press play, run one command, and the words are there — big enough to read from
across the room, arriving one at a time as they are sung.

No music library to scan, no pre-processing pass, no Python environment. One
self-contained binary that reads whatever your player is already doing.

## ⚡ Install

### 🦀 With Rust (1.88 or newer)

```bash
cargo install terminal-lyrics
```

### 📦 Without Rust — download a binary

Take the archive for your machine from the [latest
release](https://github.com/mgtaco/terminal-lyrics/releases/latest):

| | Your machine | File |
| :-: | --- | --- |
| 🍎 | macOS, Apple silicon | `lyrics-*-aarch64-apple-darwin.tar.gz` |
| 🍎 | macOS, Intel | `lyrics-*-x86_64-apple-darwin.tar.gz` |
| 🐧 | Linux | `lyrics-*-x86_64-unknown-linux-musl.tar.gz` |
| 🪟 | Windows | `lyrics-*-x86_64-pc-windows-msvc.tar.gz` |

Then unpack it and drop the binary somewhere on your `PATH`:

```bash
tar -xzf lyrics-*.tar.gz
mkdir -p ~/.local/bin && install -m755 lyrics-*/lyrics ~/.local/bin/lyrics
```

On Windows, `tar -xzf lyrics-<version>-x86_64-pc-windows-msvc.tar.gz` and move
`lyrics.exe` into any folder on your `PATH`.

> [!TIP]
> On Linux, take the **musl** build unless you know you want otherwise: it is
> statically linked, so it does not care how old your distribution's glibc is. A
> glibc build is published next to it.

### 🏛 Arch Linux

```bash
cd packaging/aur-bin && makepkg -si
```

That installs the release binary through pacman, so it uninstalls cleanly like
anything else. It is not on the AUR yet — registration there is paused while
Arch deals with a wave of automated sign-ups — but nothing about this `PKGBUILD`
needs the AUR, and it is the same one that will be published when it reopens.

<details>
<summary>🔧 …or build from a checkout</summary>

```bash
cargo build --release
mkdir -p ~/.local/bin && install -m755 target/release/lyrics ~/.local/bin/lyrics
```

(`install -D` would make the directory for you, but only on Linux: the `-D` that
macOS ships is a different flag entirely.)

</details>

Any of those is the whole install. The binary links its TLS in statically and
reaches your player through whatever the platform already provides, so nothing
needs installing alongside it — the only shared library it wants is the C
runtime you already have.

> [!NOTE]
> **macOS asks once.** Reading what Spotify is playing means sending it an Apple
> event, so the first run raises the standard Automation prompt. Allow it and it
> stops asking. If you refuse and change your mind, the switch is under System
> Settings → Privacy & Security → Automation.

## ▶️ Use

```bash
lyrics
```

It finds your player, reads what is playing, downloads synced lyrics and draws
them in block letters. Where the lyrics carry real per-word timings they appear
**one word at a time**, each word arriving as it is sung; line-level lyrics are
shown a phrase at a time.

### ⌨️ Keys

| key | |
| :-: | --- |
| `q` `Esc` | quit |
| `space` | play/pause the player |
| `,` `.` | shift lyrics 100 ms earlier / later, saved for this song |
| `0` | forget this song's shift and go back to the default |
| `f` | cycle font — block → compact → mini |
| `w` | one word at a time, or whole lines |
| `v` | show both voices at once, or one |
| `s` | cycle the highlight — never → auto → always |
| `r` | forget the cached lyrics and look them up again |

### 🎚 A song that runs early or late

Fix it once with `,` or `.` and it stays fixed. The correction belongs to the
song, not the session: it is written to disk against that track and reapplied
every time it comes on. `0` forgets it and puts the song back to the default,
and `lyrics status` says whether the song playing has a saved shift.

### 🚩 Flags worth knowing

| flag | |
| --- | --- |
| `--player spotify` | pin one player when several are running |
| `--font compact` | smaller letters — `block`, `compact` or `mini` |
| `--whole-lines` | always show the full phrase, never word by word |
| `--sweep` | highlight the sung part of whatever is on screen |
| `--single-voice` | one line at a time, even when two people are singing |
| `--lrc-dir ~/lyrics` | prefer your own `Artist - Title.lrc` files |
| `--no-network` | use only those files and the cache |
| `--offset-ms -250` | starting shift for every song you have not tuned |
| `--color-source pywal` | take one accent colour from your palette |

### 🖥 Without the TUI

| command | |
| --- | --- |
| `lyrics status` | what it sees: player, track, position, source, timing quality |
| `lyrics paths` | where the config, cache and saved offsets live |
| `lyrics fetch --artist "Radiohead" --title "Creep" --duration 238` | print one track's lyrics and exit |

`status` is the place to start when something is not working: it lists every
player and what each one is doing, then the track it settled on, the length the
player reported, whether the lyrics carry real word timings and where they came
from.

## 🎨 Colour

The default is to have no opinion. Lyrics are drawn in your terminal's own
palette entries, so they already match whatever scheme it is set to — there is
deliberately no menu of built-in themes, because picking from one would trade
that matching away. What there is instead is a source for a single accent:

```bash
lyrics --color-source pywal            # follow ~/.cache/wal/colors.json
lyrics --color-source 'fixed:#8a2be2'  # one literal colour
lyrics --color-source file:PATH        # any palette in pywal's JSON shape
```

Only the lyric text is repainted; the highlight and the status lines stay
palette colours, so the display still follows the terminal everywhere else. A
palette that cannot be read falls back to the terminal rather than refusing to
start, and `lyrics status` says when that has happened.

## ⚙️ Configuration

Nothing has to be configured, but `config.example.toml` documents every key.
Settings live in `~/.config/terminal-lyrics/config.toml`, or
`~/Library/Application Support/terminal-lyrics/config.toml` on macOS — `lyrics
paths` prints the one this build actually reads. Flags beat the file, and the
file beats the defaults.

Per-song shifts are not settings and do not live there. They are kept as JSON
next to the data directory, hold only the songs you have actually nudged, and
deleting the file simply returns every song to the default.

## 🎧 Where the lyrics come from

Your own `--lrc-dir` first, then the cache, then four sources in order — chosen
by what each is good at rather than by how often it answers, so a better-looking
set of lyrics is never passed over for a merely likelier one:

| source | timing | |
| --- | :-: | --- |
| [AMLL TTML DB](https://github.com/amll-dev/amll-ttml-db) | syllable | CC0 and community-maintained, keyed by the Spotify track ID your player already reports — one request, no guesswork, and about 2,400 entries |
| [LyricsPlus](https://github.com/ibratabian17/lyricsplus) | syllable | Apple Music's own TTML, matched on artist and title, splitting long words into pieces the display builds up in place |
| [lrcmux](https://github.com/f1nniboy/lrcmux) | word | one API in front of Musixmatch, KuGou, NetEase, Genius and YouTube Music, so it degrades rather than dies |
| [LRCLIB](https://lrclib.net) | line | millions of tracks, line-level only — the safety net |

Two of them are one person's server each, so both base URLs are configurable, as
is the list itself — which doubles as the on/off switch:

```toml
providers = ["amll", "lyricsplus", "lrcmux", "lrclib"]
```

Dropping a name skips that source; the order of the list is the order they are
asked in. One that is down is stepped over rather than failing the lookup.

## 💻 Platforms

The program is the same everywhere; only the way it finds your player differs.

| | how it finds your player | |
| --- | --- | --- |
| 🐧&nbsp;**Linux** | MPRIS over D-Bus, no subprocess anywhere | anything exposing an MPRIS interface works, which is most things |
| 🍎&nbsp;**macOS** | Spotify and Apple Music by name, once a second | the framework every now-playing tool used to call was restricted in macOS 15.4, so there is nothing left to discover players with; neither app is ever launched by this program |
| 🪟&nbsp;**Windows** | System Media Transport Controls — the API behind the media flyout | discovers players rather than knowing them by name; carries no track ID, so AMLL is skipped and the other three match on artist and title |

## 🩺 If something is off

| what you see | what to do |
| --- | --- |
| no player found | start playing something first, then check `lyrics status` — it lists every player it can see |
| it followed the wrong app | `--player spotify` pins the one you mean; browsers register idle MPRIS entries that can look like players |
| lyrics run early or late | `,` and `.` nudge by 100 ms and the correction is saved for that song; `0` undoes it |
| no lyrics at all | `r` drops the cached answer and looks again — though a track held by none of the four sources will not be found |
| words do not appear one at a time | that track came back line-level; `lyrics status` says which source answered and whether it carried real word timings |
| macOS cannot reach the player | Automation permission was refused: System Settings → Privacy & Security → Automation |

## 🔬 Under the hood

<details>
<summary><b>Choosing a player, and keeping the position</b></summary>

Without `--player`, whichever player is actually playing wins, followed by one
with a track loaded and then anything else, with the name as a stable tiebreak.
Simply taking the first name alphabetically turns out to be wrong in practice,
because browsers register idle MPRIS instances such as `chromium.instance26065`
that sort ahead of `spotify` while reporting no track at all. That ranking is
platform-neutral and shared by every backend.

The player is asked for its position once and the position is interpolated from
a monotonic clock from then on, re-anchoring whenever the player says something.
A 1 Hz read covers the players that seek without announcing it, Spotify among
them. On Linux that costs no subprocess at all; on macOS it is one short-lived
`osascript`, which is also what makes a scrub show up within a second there
despite AppleScript having no seek notification to offer.

</details>

<details>
<summary><b>Why the sources are asked in that order</b></summary>

The order is a prediction about what each source is usually good at, and it is
not taken on trust. Only word timings end the search: a provider that answers a
line at a time is held as a fallback and the next one is asked anyway, so an
Apple document that happens to be line-level cannot step over word timings
lrcmux was holding all along. The fallback is returned once nothing better has
turned up, and the first one found wins, which keeps the order deciding between
answers of equal quality.

The same reasoning reaches the cache. A word-timed hit is kept indefinitely,
since nothing better is coming for it, but a line-level hit expires after a day
like a miss does — often it only means the two hobby-run providers were
unreachable for the minute that track was playing, and keeping it forever would
make a brief outage permanent for whatever you happened to be listening to.

A lookup only fails outright when every provider failed and none of them managed
to say "not here" — losing LRCLIB's line-level answer to somebody's server being
unreachable would be the worst trade available.

</details>

<details>
<summary><b>Matching a track to lyrics</b></summary>

LRCLIB is the one that needs real matching, since it has no ID to key on. The
lookup starts with an exact query on artist, title, album and duration, retries
without the album, falls back to a scored search, and finally tries once more
with decorations like `- Remastered 2011` and `(feat. …)` stripped from the
title. Candidates more than five seconds away from the track's real length are
rejected rather than displayed out of sync, and misses are cached for a day so
that a track LRCLIB has never heard of is not queried again on every play.

</details>

<details>
<summary><b>Word timing and the highlight</b></summary>

What appears on screen follows the lyrics rather than a preference. A source
carrying real per-word timestamps — three of the four do — is shown one word at
a time, each word appearing on its own timestamp, whereas a line-level source
has nothing to split on and so is shown whole. Because that decision is made per
line rather than per file, a file that carries tags on only some of its lines
still shows the rest in full. Where a long word is timed in pieces, the pieces
build the word up in place instead of flashing a syllable on its own.

The highlight is a separate thing and is off unless you ask for it. `--sweep`
lights up the sung part of whatever is on screen, moving across the current word
in word-by-word mode and across the phrase otherwise. For line-level lyrics it
has to be interpolated from character counts between two real timestamps, which
makes it an estimate, and `lyrics status` labels it as one. It stays a display
effect throughout: nothing invented is ever written to disk.

</details>

<details>
<summary><b>Two voices at once</b></summary>

Where two people are singing at once, both are on screen. A backing vocal — the
`(ooh ooh)` behind the line, which the syllable-timed sources mark as its own
voice — sits above the line in grey and a size smaller, because it is texture
rather than the thing being read; it is drawn whole and never swept, and the
line underneath keeps the highlight. The other half of a duet is drawn full size
and lit, since it is a voice and not an echo, and while one is up the line
beneath it is shown whole rather than a word at a time.

Overlap alone is not enough to call something a duet. Across 39 files from the
AMLL database the median overlap between one line and the next is a quarter of a
second, and three quarters of them are under a second: that is one phrase's tail
running into the next one's head, not two people singing, and stacking it would
flicker a second line up a couple of times a song. So a duet has to overlap for
a full second and for half of the shorter phrase. A backing vocal needs no such
test — the source says outright that it is a second voice.

</details>

<details>
<summary><b>Drawing</b></summary>

ratatui diffs the screen buffer, so a frame that has not changed emits no bytes
at all. Lines wrap at word boundaries and the font steps down a size before
anything would be clipped, so nothing is ever truncated. Where two voices will
not fit, both step down together first, and only if even the smallest font
cannot hold them is the second voice dropped — the line being read then goes
back to the size it would have had on its own, rather than staying shrunk for
something that is no longer there.

</details>

## 🙏 Credit

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
pockets. If you lean on them, self-host: `lyricsplus_url` and `lrcmux_url` are
there for it.

## 📄 License

MIT
