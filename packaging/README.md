# Packaging and releasing

## Cutting a release

1. Bump `version` in `Cargo.toml`, then `cargo check` so `Cargo.lock` follows.
2. Commit, push, and tag:

   ```bash
   git tag v0.2.0 && git push origin v0.2.0
   ```

3. `.github/workflows/release.yml` builds five targets and attaches the archives
   to a GitHub Release. Watch it with `gh run watch`.
4. Once it lands, point `packaging/aur-bin/PKGBUILD` at the new version and
   refresh its checksum, so the packaging in this repository always matches the
   newest release:

   ```bash
   gh release view v0.2.0 --json assets \
     --jq '.assets[] | select(.name|endswith("linux-gnu.tar.gz")) | .digest'
   ```

   That digest is GitHub's own, computed when it received the file. No checksum
   files are published alongside the archives: a sidecar written by the same job
   that built the archive would only repeat this, less trustworthily.

That is the whole release. Everything below is publishing it elsewhere, and all
of it needs credentials this repository does not carry.

## crates.io

Published: <https://crates.io/crates/terminal-lyrics>. `cargo install
terminal-lyrics` is the toolchain install route.

```bash
cargo publish --dry-run   # no credentials needed; run this before every release
cargo publish             # with CARGO_REGISTRY_TOKEN set
```

The manifest carries everything crates.io requires, and `rust-version` is 1.88 —
the floor `ratatui` 0.30 imposes, not the 1.85 that edition 2024 would suggest.

### Switch this to Trusted Publishing

The crate exists now, which is the precondition: crates.io cannot hold a trusted
publisher in a pending state before a crate's first publish, so the first one had
to use an API token. That token has done its job and should be revoked.

Configure a trusted publisher on the crate's settings page — owner `mgtaco`,
repository `terminal-lyrics`, workflow `release.yml` — then a release can
authenticate over OIDC with a credential that lasts under an hour and is revoked
when the job ends, with no stored secret:

```yaml
permissions:
  id-token: write
  contents: write
steps:
  - uses: rust-lang/crates-io-auth-action@v1
    id: auth
  - run: cargo publish
    env:
      CARGO_REGISTRY_TOKEN: ${{ steps.auth.outputs.token }}
```

This is not wired into `release.yml` yet, because the job would fail on every tag
until the trusted publisher is configured.

## AUR

> **Blocked, not forgotten.** AUR account registration is paused while Arch deals
> with a wave of automated sign-ups — `HTTP 503`, not something specific to this
> project or network. It has been closed for a while rather than briefly, and no
> reopening date has been announced, so treat this as indefinite rather than
> imminent. It will be announced on
> [aur-general](https://lists.archlinux.org/mailman3/lists/aur-general.lists.archlinux.org/)
> and the [Arch news feed](https://archlinux.org/news/); the page asks explicitly
> that nobody script retries against it, and those lists will say so sooner
> anyway.
>
> None of this blocks Arch users, and the README says so: `makepkg -si` in
> `aur-bin/` installs the release binary through pacman without any AUR account.
> Everything below is what publishing adds, and it is ready to run the day an
> account exists.

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

makepkg --printsrcinfo > .SRCINFO   # must be regenerated whenever PKGBUILD changes
makepkg -si                          # build it once and check it actually runs
namcap PKGBUILD *.pkg.tar.zst        # optional but catches the usual mistakes

git add PKGBUILD .SRCINFO
git commit -m "Update to 0.2.0"
git push
```

`aur-bin/PKGBUILD` tracks the current release and carries its real checksum, so
it can be copied and built as-is; bumping it is part of cutting a release rather
than something to remember at publish time. `aur-git/PKGBUILD` has no version to
track — `pkgver()` derives it from `git describe`.

`.SRCINFO` is deliberately not committed here: `makepkg` generates it,
it must match the `PKGBUILD` exactly, and the AUR rejects a push where it does
not — so it is generated on the machine that publishes, which is also the only
machine that can verify the package builds at all.

## Other channels worth having

- **Homebrew tap** — `mgtaco/homebrew-tap` with a formula pointing at the macOS
  archives. Covers `brew install mgtaco/tap/terminal-lyrics`.
- **Scoop bucket** — the Windows equivalent, pointing at the msvc archive.
- **nixpkgs** — `buildRustPackage` against the release tarball.
