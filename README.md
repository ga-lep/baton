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

## Configuration

Baton reads `~/.config/baton/config.toml` (or `$XDG_CONFIG_HOME/baton/config.toml`;
set `BATON_CONFIG` to use another path). Start from the annotated example:

```sh
mkdir -p ~/.config/baton
curl -fsSL https://raw.githubusercontent.com/ga-lep/baton/main/examples/config.toml \
  -o ~/.config/baton/config.toml
```

Then edit the `[[projects]]` entries to point at your repos. See
[examples/config.toml](examples/config.toml) for every option.

Minimal example

```toml
# ~/.config/baton/config.toml
update_check = true

[[projects]]
name = "my-repo"
path = "/home/user/projects/my-repo"
hook = "baton hook"
```

## Copying text from a session

Baton captures the mouse, so a plain drag does not select text, and the sidebar
and borders would end up in the copy. Use fullscreen instead:

- Press `z` or `Alt-z` in normal mode, or `Alt-z` in focus mode, to toggle
  fullscreen. `Esc` in normal mode also leaves it.
- The session pane fills the terminal with no border or sidebar, and mouse
  capture is off, so a plain drag with the terminal's own selection copies just
  the session text.
- The bottom bar stays, showing the mode and the fullscreen key. Stop the drag
  above it, or the bar text is copied too.
- Claude receives no mouse events while fullscreen (no wheel scroll or clicks).
  Toggle back to use them.
- Lines that Claude soft-wrapped still copy as separate lines, one per screen
  row. Wrapping follows the terminal width.
- In focus mode `Alt-z` is taken by baton and no longer reaches the session.
- The keys are remappable as the `fullscreen` action under `[keybindings.normal]`
  and `[keybindings.focus]`. Setting `fullscreen` replaces that mode's default
  keys, so list every key you want (for example `["z", "alt-z"]`).

## Updates

Run `baton version --check` to see whether a newer release exists.

Baton also checks occasionally in the background. To opt out, set
`BATON_NO_UPDATE_CHECK=1` in the environment, or `update_check = false` in the
config file.

## License

Baton is licensed under the MIT License. See `LICENSE`.
