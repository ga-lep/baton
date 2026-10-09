# Baton

Baton is a terminal UI (Linux only) for running several Claude
Code sessions side by side. A background daemon owns one interactive `claude`
session per configured repo, tracks each session's status, keeps sessions
alive across TUI restarts, shows usage and estimated cost, and sends desktop
notifications when a session needs attention. See [docs/SPEC.md](https://github.com/ga-lep/baton/blob/main/docs/SPEC.md)
for details.

## Install

1. Download the `x86_64-unknown-linux-musl` archive (recommended) from
   [GitHub Releases](https://github.com/ga-lep/baton/releases), together with
   its checksum file.
2. Verify it, in the download directory, with either
   `sha256sum -c baton-<tag>-x86_64-unknown-linux-musl.tar.gz.sha256` or
   `sha256sum -c SHA256SUMS --ignore-missing`. Checksums prove the download
   is intact, not who built it.
3. Extract it and put `baton` on your `PATH`, for example in `~/.local/bin`.

See [docs/RELEASING.md](https://github.com/ga-lep/baton/blob/main/docs/RELEASING.md)
for details.

## Updates

Run `baton version --check` to see whether a newer release exists.

Baton also checks occasionally in the background. To opt out, set
`BATON_NO_UPDATE_CHECK=1` in the environment, or `update_check = false` in the
config file.

## License

Baton is licensed under the MIT License. See `LICENSE`.
