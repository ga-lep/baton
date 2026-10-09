# Releasing and installing Baton

## Cutting a release

The version source of truth is `[workspace.package] version` in `Cargo.toml`.

1. Bump the version in `Cargo.toml`.
2. Run `cargo check` to refresh `Cargo.lock` (release builds use `--locked`).
3. Merge the change to `main` through a pull request.
4. Tag the merged commit and push the tag:

   ```
   git tag -a vX.Y.Z -m vX.Y.Z <main-sha>
   git push origin vX.Y.Z
   ```

Pushing a tag that matches `v[0-9]+.[0-9]+.[0-9]+*` starts
`.github/workflows/release.yml`. A tag with a suffix such as `v0.2.0-rc.1`
publishes a pre-release. The jobs run in this order:

1. `gate`: the CI workflow (fmt, clippy, tests).
2. `verify`: fails unless the tag minus its leading `v` equals the workspace
   version (read with `cargo metadata --no-deps --format-version 1`).
3. `build`: builds `x86_64-unknown-linux-musl` and `x86_64-unknown-linux-gnu`,
   smoke-tests each binary (`--version`, `baton version`, and "statically
   linked" for musl) and uploads the archives.
4. `publish`: tag refs only, the only job with `contents: write`. It writes
   `SHA256SUMS` and runs `gh release create` with `--verify-tag` and
   `--generate-notes`.

The workflow also runs in build-only mode on pull requests that touch
`release.yml` and on `workflow_dispatch`. It builds and uploads workflow
artifacts but never publishes, and it skips the tag comparison.

## Artifacts

Each release contains, per target:

- `baton-<tag>-<target>.tar.gz`, holding one directory
  `baton-<tag>-<target>/` with `baton`, `LICENSE` and `README.md`;
- `baton-<tag>-<target>.tar.gz.sha256`, in `sha256sum` format.

It also contains one `SHA256SUMS` file covering all archives.

Which archive to pick:

- `x86_64-unknown-linux-musl` is recommended. It is statically linked and runs
  on any x86_64 Linux.
- `x86_64-unknown-linux-gnu` is dynamically linked and needs glibc 2.39 or
  newer.

No aarch64 build is provided.

## Verifying a download

Download the archive and `SHA256SUMS` (or the archive's own `.sha256`) into the
same directory, then run one of:

```
sha256sum -c SHA256SUMS --ignore-missing
sha256sum -c baton-<tag>-<target>.tar.gz.sha256
```

Note that the checksums prove integrity only (the download was not corrupted).
They come from the same release as the archives, so they do not prove
authenticity: anyone able to replace an archive could replace its checksum too.

## Installing

```
tar xzf baton-<tag>-x86_64-unknown-linux-musl.tar.gz
mkdir -p ~/.local/bin
install -m 755 baton-<tag>-x86_64-unknown-linux-musl/baton ~/.local/bin/baton
baton --version
```

Make sure `~/.local/bin` is on your `PATH`.

## Update check

`baton version --check` asks GitHub whether a newer release exists. `baton
doctor` shows a version line and the TUI shows a non-blocking notice. To turn
the background check off, set `BATON_NO_UPDATE_CHECK=1` in the environment or
`update_check = false` in the config file. The `hook`, `statusline` and daemon
paths never check.

The repository must be made public before the update check and the public
downloads work. Until then `baton version --check` reports
`no public release found`, `baton doctor` shows a WARN, and the TUI shows
nothing, the same as when offline.

## License

Baton is MIT licensed. `LICENSE` and `README.md` ship in every archive.
