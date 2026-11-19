<div align="center">

# <img src="docs/logo.svg" width="40" align="top" alt=""> terminal-lyrics

**Giant block lyrics in your terminal, synced to whatever is playing.**

[![crates.io downloads](https://img.shields.io/crates/d/terminal-lyrics?style=flat-square&label=crates.io%20downloads&color=B7410E&logo=rust&logoColor=white)](https://crates.io/crates/terminal-lyrics)
[![GitHub release downloads](https://img.shields.io/github/downloads/mgtaco/terminal-lyrics/total?style=flat-square&label=release%20downloads&color=2EA44F&logo=github&logoColor=white)](https://github.com/mgtaco/terminal-lyrics/releases)
[![platform: Linux | macOS | Windows](docs/badges/platform.svg)](#platforms)
[![Seven lyrics sources](https://img.shields.io/badge/lyrics-7_synced_sources-0EA5E9?style=flat-square)](#how-it-works)
[![MIT licence](https://img.shields.io/github/license/mgtaco/terminal-lyrics?style=flat-square&color=8A2BE2)](LICENSE)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/demo-dark.gif">
  <img alt="Word-timed lyrics appearing one word at a time, a long word building up syllable by syllable, first in lowercase and then in capitals" src="docs/demo-light.gif">
</picture>

<sub>Rendered by the program itself.</sub>

</div>

Run `lyrics`, play something, and the lyrics appear one word at a time. It follows your music player and pulls synced lyrics from seven sources.

## Install

**Linux or macOS**

```bash
curl -fsSL https://raw.githubusercontent.com/mgtaco/terminal-lyrics/main/install.sh | sh
```

**With a Rust toolchain** (1.88 or newer)

```bash
cargo install terminal-lyrics
```

**Windows**: download `lyrics-*-x86_64-pc-windows-msvc.tar.gz` from the [latest release](https://github.com/mgtaco/terminal-lyrics/releases/latest), unpack it with `tar -xzf` (built into Windows 10 and later) and put `lyrics.exe` somewhere on your `PATH`.

Then run `lyrics` and play something. On macOS the first run will ask to let your terminal control Spotify or Music, which is how it reads what is playing; allow it once and it will stop asking. If you refused, the switch is under System Settings → Privacy & Security → Automation.

<details>
<summary><b>About the install script, and other ways to install</b></summary>
<br>
<table><tr><td>

[`install.sh`](install.sh) grabs the archive for your machine from the latest release, checks its SHA-256 and puts `lyrics` in `~/.local/bin`. It will tell you if that is not on your `PATH` yet. Run it again to update, and delete `~/.local/bin/lyrics` to uninstall. Set `LYRICS_VERSION=0.3.0` for a particular release or `LYRICS_INSTALL_DIR` for another directory, on the `sh` side of the pipe. To read it first:

```bash
curl -fsSLO https://raw.githubusercontent.com/mgtaco/terminal-lyrics/main/install.sh
```

The binary is self-contained, so there is nothing else to install.

**By hand**, take the archive for your machine from the [latest release](https://github.com/mgtaco/terminal-lyrics/releases/latest):

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

Not sure which Linux build? Take musl: it is statically linked, so it will run however old your glibc is.

**On Arch**, `packaging/aur-bin/` has a `PKGBUILD` that installs the release binary through pacman:

```bash
cd packaging/aur-bin && makepkg -si
```

It will go up on the AUR once account registration there reopens.

**From a checkout**:

```bash
cargo build --release
mkdir -p ~/.local/bin && install -m755 target/release/lyrics ~/.local/bin/lyrics
```

</td></tr></table>
<br>
</details>

## Use

```bash
lyrics
```

| key | |
| --- | --- |
| `?` | list every key with its current setting |
| `q` `Esc` | quit (`Esc` closes the key list first) |
| `space` | play/pause the player |
| `,` `.` | shift lyrics 100 ms earlier / later, saved for this song |
| `<` `>` | the same in whole seconds, for a song that is badly out |
| `0` | forget this song's shift and go back to the default |
| `f` | cycle font: block → compact → mini |
| `c` | switch between capitals and lowercase |
| `w` | one word at a time, or whole lines |
| `h` | cycle the highlight: never → auto → always |
| `v` | [cava](https://github.com/karlstav/cava)'s spectrum bars under the lyrics, on or off |
| `s` | hide or show the lyrics source in the bottom-left corner |
| `t` | hide or show the song's name in the bottom-right corner |
| `r` | forget the cached lyrics and look them up again |

What you pick with `f`, `c`, `w`, `h`, `v`, `s` and `t` is saved, so the next run will start the way you left it. Shifts are saved per song, so once you fix one it will stay fixed. Closing the player will not close the lyrics; they will pick up the next player that starts.

Handy flags (`lyrics --help` lists them all):

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
| `--hide-title` | leave the bottom-right corner empty instead of naming the song |

<details>
<summary><b>Per-song sync</b></summary>
<br>
<table><tr><td>

Nudge a song with `,` and `.` in 100 ms steps, or with `<` and `>` in whole seconds when it is way out. The shift comes back every time that song plays with lyrics from the same provider. Each provider gets its own shift, since they're rarely off by the same amount, and every untuned song or provider starts from `offset_ms` in the config. `0` puts it back on the default.

Shifts are kept in `~/.local/share/terminal-lyrics/offsets.json` (`~/Library/Application Support/terminal-lyrics/offsets.json` on macOS). Only songs you have nudged end up in it, listed by provider, so it is easy to edit by hand, and deleting it resets them all. `lyrics paths` prints where it is, and `lyrics status` shows whether the current song has a shift for the provider it's using.

</td></tr></table>
<br>
</details>

<details>
<summary><b>Visualizer</b></summary>
<br>
<table><tr><td>

With [cava](https://github.com/karlstav/cava) installed, `v` (or `--visualizer`, or `visualizer = true` in the config) draws its spectrum bars under the lyrics, in the same faint colour as the status line. It reads the input settings from your own `~/.config/cava/config`, so if cava already listens to one player, these bars will too. If that player isn't running, the bars stay flat until it is, rather than picking up your mic. Without cava, nothing is drawn.

</td></tr></table>
<br>
</details>

### Without the TUI

```bash
lyrics status                 # player, track, position, and which source matched
lyrics fetch --artist "Radiohead" --title "Creep" --duration 238
lyrics fetch --artist "Daft Punk" --title "Get Lucky" --json   # the full answer, as JSON
lyrics paths                  # where the config, cache and saved offsets live
```

If something is not working, start with `lyrics status`. It shows every player and what it is doing, the track it picked, the length the player reported, whether the lyrics have word timings and where they came from.

## Configuration

Settings live in `~/.config/terminal-lyrics/config.toml` (`~/Library/Application Support/terminal-lyrics/config.toml` on macOS), and `lyrics paths` will print the one in use. [`config.example.toml`](config.example.toml) lists every key. Flags beat the file, and the file beats the defaults. The `f`, `c`, `w`, `h`, `v`, `s` and `t` keys only touch their own line, so your comments and layout stay put.

<details>
<summary><b>Colours</b></summary>
<br>
<table><tr><td>

By default the lyrics use your terminal's palette, so they will match whatever theme you already have. If you want one accent colour instead:

```bash
lyrics --color-source pywal            # follow ~/.cache/wal/colors.json
lyrics --color-source 'fixed:#8a2be2'  # one literal colour
lyrics --color-source file:PATH        # any palette in pywal's JSON shape
```

That only repaints the lyrics (background vocals and status lines included); the sweep highlight stays a palette colour. If the palette cannot be read it will fall back to the terminal, and `lyrics status` will say so.

Background vocals use the terminal's faint attribute, which holds up on translucent and blurred backgrounds. If yours draws it too weak, too strong or not at all, set `dim_color = "#rrggbb"` (or `--dim-color`).

</td></tr></table>
<br>
</details>

## Platforms

| | Follows | |
| :-: | --- | --- |
| <img src="docs/icons/linux.svg" width="15" alt=""> | any MPRIS player | over D-Bus, with no subprocess anywhere |
| <img src="docs/icons/apple.svg" width="15" alt=""> | Spotify and Apple Music | through their scripting dictionaries, once a second |
| <img src="docs/icons/windows.svg" width="15" alt=""> | anything in the system media controls | the media flyout's API, as things change |

<details>
<summary><b>Details per platform</b></summary>
<br>
<table><tr><td>

Only the way it finds your player changes between platforms. That all lives in `src/player/`, and each binary carries just its own backend.

**Linux**: MPRIS over D-Bus, with no subprocess. Changes and seeks arrive as signals, and anything with an MPRIS interface works, which is most things.

**macOS**: Spotify and Apple Music through their scripting dictionaries, polled once a second. It knows those two by name because macOS 15.4 locked down the MediaRemote framework other now-playing tools used. It will never launch either app; if one is not running, it just will not show up.

**Windows**: the System Media Transport Controls, the API behind the media flyout in the volume panel. Changes arrive as they happen, with a read once a second as backup, and any app that registers a session can be followed.

</td></tr></table>
<br>
</details>

## How it works

### Choosing a player

Without `--player`, whatever is actually playing wins, then anything with a track loaded, then the rest, by name. Going by name alone would not work, since browsers leave idle MPRIS instances like `chromium.instance26065` around that sort ahead of `spotify`. If the wrong player gets picked, `lyrics status` will show what each one is doing.

### Position

The player is asked once and the position is counted forward from a clock, re-syncing whenever the player says something. A read once a second catches players that seek without saying so, like Spotify. On macOS that read is a quick `osascript`, which is also how a scrub shows up within a second there.

### Sources

Before going online it checks your `--lrc-dir` files and, for a local file the player hands over as a `file://` URL, a lyrics file next to it: `song.lyricsfile.yaml`, `song.ttml`, `song.lrc` or `song.LRC`, in that order. Then six providers:

| source | timing | |
| --- | :-: | --- |
| [BetterLyrics](https://better-lyrics.boidu.dev) | syllable | Apple Music's TTML, matched on title, artist, album and length |
| [Unison](https://unison.betterlyrics.org) | syllable | BetterLyrics' community-submitted lyrics, TTML or enhanced LRC |
| [LyricsPlus](https://github.com/ibratabian17/lyricsplus) | syllable | JSON with per-syllable timing and backing vocals marked word by word |
| [LRCLIB](https://lrclib.net) | line | millions of tracks, line-level nearly always, now and then a word-timed Lyricsfile |
| YouTube Music | line | the lyrics YouTube Music shows, found through its own search |
| KuGou | word | its KRC format, with baked-in translations dropped. Its timings are often off, so it goes last and only wins when nothing else has word timings |

```toml
providers = ["betterlyrics", "unison", "lyricsplus", "lrclib", "youtubemusic", "kugou"]
```

Remove a name to turn that provider off; the order is the priority. They are all asked at once, and better timing wins first (word over line over plain), then priority. So a quick line-level answer from LRCLIB will wait while a word-timed provider is still out. Once there is a clear winner it shows up after two seconds, but a slower provider with better timing can still replace it within the twelve-second lookup. Providers that are down get skipped, and lyrics with no timing at all are not shown.

Each provider caches per track, on artist, title, album and length. Hits are kept for ninety days and misses are retried after fourteen, as is a line-level hit from a provider that usually has word timings. `r` clears the current song from the cache and looks again.

### Matching LRCLIB

It tries an exact match on artist, title, album and length first. If that comes back plain or empty, it searches by title and takes the best record with the same artist within eight seconds of the track's length: word timing over line timing over plain, then the closest length.

### Word timing

Lyrics with per-word timestamps are shown one word at a time, and line-level lyrics are shown whole. That is decided per line, so a file with word tags on only some lines still works. A long word timed in pieces builds up in place instead of flashing one syllable at a time.

### Highlight

`--sweep` lights up the sung part of whatever is on screen: the current word in word-by-word mode, the whole phrase otherwise. For line-level lyrics it is estimated from character counts, and `lyrics status` will say so. It is display-only and never written to disk.

`auto`, the default, only sweeps when it has word timings to place it and a whole line to place it on. So it stays off in word-by-word mode, where one word at a time already shows where you are. `--sweep` turns it on anyway and sweeps the syllables of the word on screen.

### Two voices

When two people sing at once, both go on screen. A backing vocal (the `(ooh ooh)` behind the line) sits above it, faint and a size smaller, and steps through its own words but never gets the highlight. The other half of a duet is drawn full size: in word-by-word mode both voices show their own word side by side, and the sweep runs across both. If the partner has no word timings, both are shown as whole phrases.

Lines that just overlap a bit are not a duet. Across 39 files from the AMLL database the median overlap between one line and the next is a quarter of a second, which is just one phrase running into the next. So a duet has to overlap for a full second and for half of the shorter phrase. Backing vocals skip that test, since the source marks them.

### Drawing

Letters are pixel art made of half blocks (`▀ ▄ █`), which come out square on most terminals. Capitals, lowercase, digits and punctuation are hand-drawn, and accents are stamped on top. That covers Latin-script languages (Vietnamese included), Cyrillic for Russian, Ukrainian, Belarusian, Bulgarian, Serbian and Macedonian, and Greek. Chinese, Japanese and Korean come from a 10-pixel CJK font the same height as the capitals, and lines without spaces can wrap between any two characters, but never before a small kana. A line only grows a row for accents or descenders and the capitals never move, so a word with a `g` in it will not bob. Scripts with no art (Arabic, Hebrew, Thai) are shown as plain text.

Nothing is ever cut off: lines wrap at word boundaries, and the font steps down a size before anything would clip. If two voices will not fit, both shrink together first, and the second voice only gets dropped if even the smallest font is too big. The main line then goes back to its normal size.

## Limitations

* Not every song has word timings. An obscure track can still come back line-level from LRCLIB or YouTube Music, and `lyrics status` will tell you which source you got.
* Most of the sources are community-run, and some are unofficial APIs that can change without warning.
* If none of the seven has a song, it will not be found, and `r` will not magic it up.
* Only players the platform exposes are visible: MPRIS on Linux, the media controls on Windows, and just Spotify and Apple Music on macOS.
* macOS needs Automation permission for your player. If you refuse, it will tell you and point you at the setting.

## Credit

The idea and the big block-letter look come from [tacos-terminal-lyrics](https://github.com/tacoproz1/tacos-terminal-lyrics) by tacoproz1. This is a separate program written from scratch in Rust, with no music library to scan.

Lyrics come from BetterLyrics, Unison, [LyricsPlus](https://github.com/ibratabian17/lyricsplus), KuGou, [LRCLIB](https://lrclib.net) and YouTube Music. None of them are affiliated with this project. Third-party licences are in [licenses/](licenses/).

The Chinese, Japanese and Korean characters are from [Fusion Pixel Font](https://github.com/TakWolf/fusion-pixel-font) by TakWolf (SIL Open Font License 1.1), converted to the program's own format. The Cyrillic letters were drawn by [deepsytxt](https://github.com/deepsytxt).

The platform marks in `docs/icons/` are based on [Simple Icons](https://simpleicons.org) (CC0).

## License

MIT
