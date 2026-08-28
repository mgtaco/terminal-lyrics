# Packaging and releasing

## Cutting a release

1. Bump `version` in `Cargo.toml`, then `cargo check` so `Cargo.lock` follows.
2. Commit, push, and tag:

   ```bash
   git tag v0.2.0 && git push origin v0.2.0
   ```

3. `.github/workflows/release.yml` builds five targets and attaches each archive
   plus a `.sha256` to a GitHub Release. Watch it with `gh run watch`.

That is the whole release. Everything below is publishing it elsewhere, and all
of it needs credentials this repository does not carry.

## crates.io

```bash
cargo publish --dry-run   # no credentials needed; run this first
cargo login               # once, with a token from crates.io/settings/tokens
cargo publish
```

`cargo install terminal-lyrics` works once this lands. The manifest already
carries everything crates.io requires, and `rust-version` is 1.88 — the floor
`ratatui` 0.30 imposes, not the 1.85 that edition 2024 would suggest.

## AUR

Two packages, both here:

| directory | package | what it does |
| --- | --- | --- |
| `aur-bin/` | `terminal-lyrics-bin` | downloads the release binary; no toolchain needed |
| `aur-git/` | `terminal-lyrics-git` | builds from `main` |

Publishing one, per package:

```bash
git clone ssh://aur@aur.archlinux.org/terminal-lyrics-bin.git
cp packaging/aur-bin/PKGBUILD terminal-lyrics-bin/
cd terminal-lyrics-bin

# -bin only: point at the new version and take its checksum from the release.
sed -i 's/^pkgver=.*/pkgver=0.2.0/' PKGBUILD
updpkgsums

makepkg --printsrcinfo > .SRCINFO   # must be regenerated whenever PKGBUILD changes
makepkg -si                          # build it once and check it actually runs
namcap PKGBUILD *.pkg.tar.zst        # optional but catches the usual mistakes

git add PKGBUILD .SRCINFO
git commit -m "Update to 0.2.0"
git push
```

Both `PKGBUILD`s carry `pkgver=0.0.0` as a placeholder rather than a stale real
version, so an un-updated one fails loudly instead of quietly packaging an old
release. `.SRCINFO` is deliberately not committed here: `makepkg` generates it,
it must match the `PKGBUILD` exactly, and the AUR rejects a push where it does
not — so it is generated on the machine that publishes, which is also the only
machine that can verify the package builds at all.

## Other channels worth having

- **Homebrew tap** — `mgtaco/homebrew-tap` with a formula pointing at the macOS
  archives. Covers `brew install mgtaco/tap/terminal-lyrics`.
- **Scoop bucket** — the Windows equivalent, pointing at the msvc archive.
- **nixpkgs** — `buildRustPackage` against the release tarball.
