# terminal-lyrics

Rules for working in this repo. Keep this file to rules: what the app does and
how it is built belongs in the README and the code, where it is kept current.
A line here that only describes the app goes stale the first time it changes.

Repo: https://github.com/mgtaco/terminal-lyrics (`origin`, branch `main`).

## Writing code

- Keep it simple.
- Do not edit code in response to a question.
- Comment only what the code cannot say: a non-obvious why, a trap, a
  workaround. Never narrate what the code does or restate a name. Keep it to a
  line or two, and delete useless or paragraph comments you come across.

## Checks

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

All three must pass before a change is called done; CI runs the same on Linux,
macOS and Windows. Tests need no terminal or player: anything that cannot be
pure goes through the scripted fake player (`src/player/fake.rs`) or a parse
function that takes the platform's raw output, so add new tests the same way.

Useful while working (see `examples/`):

```bash
cargo run --example pump_dump -- 15     # dump the live player event stream
cargo run --example demo_gif            # re-render the README demo GIFs
cargo run --example font_sheet          # print every glyph, to check font art
lyrics status                           # what it sees: player, track, source
```

Re-render the demo GIFs when a change alters what they show.

## Constraints

- Tests must never shell out to `osascript`: it raises the macOS Automation
  prompt. Keep platform decisions in functions that parse the probe's output.
- `src/lyrics/` is a port of DankMaterialShell's `dankgo/lyrics` (MIT, notice
  in `licenses/`) and shares its cache. Keep the lookup, request shape and cache
  format compatible with upstream, but do not name it in user-facing docs or
  code comments.
- Additions upstream lacks are fine and stay; just keep them out of the shared
  cache's track key.
- Nothing estimated or interpolated (e.g. the sweep highlight) is written to
  disk.
- A new config key or flag goes in `config.example.toml` and the README in the
  same change.
- Release archive names come from `.github/workflows/release.yml`;
  `install.sh` and `packaging/aur-bin/PKGBUILD` download them by name, so
  change all three together.
- After pushing a release tag, `git pull` before the next push: the release
  workflow commits the PKGBUILD bump to `main` once the release is up.
- When trying `install.sh`, set `LYRICS_INSTALL_DIR` to a scratch directory.
  Its default, `~/.local/bin`, is where the dev build lives, and a test run
  replaces it with the last release.

## After making changes

1. **Build and install** every time a change works, so the `lyrics` on PATH
   has the change and can be tried:

   ```bash
   cargo build --release
   install -m755 target/release/lyrics ~/.local/bin/lyrics
   ```

2. **Commit and push** to `origin main` only once the user has tried it and
   says it is right. A change often gets adjusted after a first look, so
   finish by saying it is installed and ready to try, and wait for the
   go-ahead rather than pushing straight away.

3. **Commit messages**: an imperative subject saying what changed for the
   user ("Keep running when the player closes"), no prefix, then a body in
   plain prose saying what it used to do and what it does now.
