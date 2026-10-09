# Baton

Baton is a terminal UI (Linux only) for running several Claude
Code sessions side by side. A background daemon owns one interactive `claude`
session per configured repo, tracks each session's status, keeps sessions
alive across TUI restarts, shows usage and estimated cost, and sends desktop
notifications when a session needs attention. See [docs/SPEC.md](https://github.com/ga-lep/baton/blob/main/docs/SPEC.md)
for details.

## Install or update

```sh
(cd "$(mktemp -d)" && curl -fsSL --remote-name-all "https://github.com/ga-lep/baton/releases/latest/download/{baton-x86_64-unknown-linux-musl,SHA256SUMS}" && sha256sum -c --ignore-missing --quiet SHA256SUMS && install -Dm755 baton-x86_64-unknown-linux-musl ~/.local/bin/baton) && baton --version
```

It downloads the latest static Linux x86_64 binary, checks it against the
release's `SHA256SUMS`, and installs it as `~/.local/bin/baton` (make sure that
directory is on your `PATH`). Run the same command again to update. A daemon
that was already running keeps the old version until you run
`baton daemon stop`; your sessions resume when you reopen `baton`.

The checksum only proves the download is intact, not who built it. See
[docs/RELEASING.md](https://github.com/ga-lep/baton/blob/main/docs/RELEASING.md)
for the other build and manual steps.

## Updates

Run `baton version --check` to see whether a newer release exists.

Baton also checks occasionally in the background. To opt out, set
`BATON_NO_UPDATE_CHECK=1` in the environment, or `update_check = false` in the
config file.

## License

Baton is licensed under the MIT License. See `LICENSE`.
