# Baton — Spec v1

A TUI that runs and orchestrates several interactive Claude Code sessions in one terminal screen.

Status: **draft, pre-implementation** · 2026-10-08

---

## 1. Goals

1. **One screen for everything.** Every live Claude Code session for the current work is visible and reachable from one TUI. No juggling terminal tabs.
2. **Real Claude, embedded.** Each session is the native interactive `claude` UI running in a PTY inside Baton's main panel, so slash commands, permission prompts and everything else work unchanged.
3. **Projects as the unit of work.** A project is a named group of repos in config (e.g. `loop` = alaloop, powerloop, hyperloop). Opening it starts one `claude` per repo.
4. **Know who needs you.** Every session shows a live status (running / needs permission / waiting for you / exited). A single key jumps to the next session that needs attention, and a desktop notification fires when you aren't looking.
5. **Sessions outlive the UI.** A background daemon owns the sessions. Quitting or crashing the TUI doesn't stop them; reopening re-attaches.
6. **Multiple Claude profiles.** Work vs personal (different `CLAUDE_CONFIG_DIR`), selectable per project or repo.

Target workflow: **2–4 hands-on sessions at a time**, steered closely, one per repo, independent of each other.

## 2. Non-goals (v1)

| Out of v1 | Why / when |
|---|---|
| Git worktrees, branches, diff/staging panels | One session per repo in its normal checkout. Use your IDE for git. |
| Broadcast prompts, cross-session shared context | Sessions are independent. |
| Task queue / batch fire-and-forget runs | Doesn't match a hands-on workflow. |
| Quick-approve permissions from the sidebar | Approve inside the session pane. Candidate for v2. |
| Custom chat view on `-p --output-format stream-json` | We embed the native UI instead. |
| History browser / pick a past conversation | v1 always continues the last conversation. v2. |
| Ad-hoc sessions outside config, rename, close/stop from TUI | v2. (Stopping is covered by `baton daemon stop` and restart.) |
| Theming | Hardcoded palette in v1. |
| macOS / Windows | Linux only. macOS may mostly work but is untested. |
| Multiple TUI clients attached at once, remote access | Single local client. |

## 3. MVP feature list

**Projects & sessions**
- Projects and repos declared in `config.toml`, all shown in the sidebar. Unopened projects are dimmed.
- Opening a project starts one session per repo (if not already live) with `claude --resume <last-known-id>`, falling back to `claude --continue` and then a fresh session.
- **Restart** a session: kill its process and relaunch with `--resume <session_id>` to continue the same conversation.
- A session that exits stays in the list as `exited`. Restart brings it back.

**Embedded terminal**
- The selected session renders live in the main panel, with full colors, Unicode and cursor.
- Focus mode sends all keystrokes to `claude`. `Ctrl-\` leaves focus mode.
- Scrollback (configurable cap, default 10k lines) viewable from normal mode.
- The PTY resizes with the panel.

**Status & attention**
- Status per session, driven by Claude Code hooks plus process supervision (see §6).
- "Needs attention" = needs permission, or finished its turn and you haven't looked yet.
- `n` / `Alt-n` jumps to the next session needing attention.
- Desktop notification (`notify-send` via D-Bus) when a session starts needing attention and either the TUI isn't attached, the terminal isn't focused, or that session isn't the one on screen.

**Session info panel** (selected session)
- Project / repo path, profile, model, status, uptime
- Claude session ID
- Context window usage % and token totals (in / out / cache)
- Estimated cost (from token counts × configured price table; labeled "est.")

**Other**
- `e` opens the session's repo in the configured editor/IDE (detached GUI process).
- Config: projects, repos, profiles, per-repo extra `claude` args, editor command, notifications toggle, **remappable keybindings**.
- `q` quits the TUI only. Sessions keep running in the daemon.

## 4. UX

### Layout

```
┌ Projects ───────────────────┐┌ powerloop · work · ◐ needs permission ────────────────┐
│▾ loop                       ││                                                       │
│  1 ● alaloop      running   ││  ⏺ Bash(cargo test -p engine)                         │
│  2 ◐ powerloop    permission││                                                       │
│  3 ✓ hyperloop    your turn ││  Allow Bash(cargo test -p engine)?                    │
│▸ infra            (closed)  ││  ❯ 1. Yes                                             │
│▸ blog             (closed)  ││    2. Yes, and don't ask again for cargo test         │
│                             ││    3. No, and tell Claude what to do differently      │
│                             ││                                                       │
│                             ││              (live embedded claude PTY)               │
├ Session ────────────────────┤│                                                       │
│ ~/code/powerloop            ││                                                       │
│ profile  work               ││                                                       │
│ model    opus               ││                                                       │
│ up       1h12m              ││                                                       │
│ context  ███████░░░ 68%     ││                                                       │
│ tokens   1.2M in / 84k out  ││                                                       │
│ cost     ~$4.10 (est.)      ││                                                       │
│ id       7f3c…a91e          ││                                                       │
└─────────────────────────────┘└───────────────────────────────────────────────────────┘
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  ? help  q quit
```

In focus mode, the main panel border is highlighted and the bottom bar reads
`FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session`.

### Status badges

| Badge | State | Meaning |
|---|---|---|
| `…` | starting | Process spawned, no `SessionStart` hook yet |
| `●` | running | Claude is working (prompt submitted / tool activity) |
| `◐` | permission | Blocked on a permission prompt — **attention** |
| `✓` | your turn | Turn finished, not yet viewed — **attention** |
| `○` | idle | Turn finished and you've seen it |
| `✗` | exited | Process ended (exit code shown in info panel) |
| `?` | unknown | Running, but no hook events received (hooks failing?) |

### Default keybindings (all remappable)

**Normal mode** (sidebar has focus)

| Key | Action |
|---|---|
| `j` / `k`, `↓` / `↑` | Move selection (projects and sessions) |
| `Enter` / `l` | On session: focus its pane · on closed project: open it |
| `o` | Open project under cursor (start its sessions) |
| `Space` | Collapse / expand the project under the cursor (`▸ name  (N)` while collapsed; highlighted if a hidden session needs attention). Jumping to a hidden session (`n`, `1`–`9`) expands it |
| `1`..`9` | Select session N of the current project |
| `n` | Jump to next session needing attention |
| `r` | Restart selected session (`--resume` same conversation) |
| `e` | Open repo in editor |
| `Ctrl-u` / `Ctrl-d`, `PgUp` / `PgDn` | Scroll session pane scrollback |
| `G` | Snap pane back to live bottom |
| `?` | Help overlay |
| `q` | Quit TUI (sessions keep running) |

**Focus mode** (session pane has focus): every key goes to `claude` except

| Key | Action |
|---|---|
| `Ctrl-\` | Back to normal mode |
| `Alt-n` | Next attention session (stays in focus mode) |
| `Alt-1`..`Alt-9` | Switch to session N (stays in focus mode) |

Viewing a session in the main panel marks its `✓ your turn` as seen (→ `○ idle`).

## 5. Configuration

`~/.config/baton/config.toml`

```toml
editor = "code {path}"          # {path} = repo dir; spawned detached
notifications = true
scrollback_lines = 10000
# statusline = "~/bin/my-statusline.sh"   # your own status line inside sessions (see below)

[profiles.work]
command = "claude"

[profiles.personal]
command = "claude"
env = { CLAUDE_CONFIG_DIR = "~/.claude-personal" }

[[projects]]
name = "loop"
profile = "work"                 # default for the project's repos
repos = [
  { path = "~/code/alaloop" },
  { path = "~/code/powerloop", args = ["--model", "opus"] },
  { path = "~/code/hyperloop", profile = "personal" },   # per-repo override
]

[[projects]]
name = "blog"
profile = "personal"
repos = [{ path = "~/code/blog" }]

[pricing]                        # $/MTok, used for the "est." cost only
default = { input = 3.0, output = 15.0, cache_read = 0.3, cache_write = 3.75 }
# per-model overrides keyed by model id prefix

[keybindings.normal]
next_attention = "n"
restart = "r"
# …

[keybindings.focus]
unfocus = "ctrl-\\"
next_attention = "alt-n"
```

Baton sets Claude's status line to `baton statusline` (via `--settings`, which overrides a `statusLine` in Claude's own settings). It relays the status line JSON to the daemon, which shows the subscription quota (`rate_limits`: 5-hour and weekly windows, Pro/Max only) in the info panel of every session of the same profile. It then runs the configured `statusline` command with the same JSON on stdin and prints its output, or prints nothing.

A session is identified by `(project, repo path)`. Changing config while the daemon runs leaves live sessions alone. New projects/repos show up on TUI reload (`baton` reads config on attach; the daemon re-reads on `OpenProject`).

## 6. Architecture

### Processes

```
 ┌──────────────┐  unix socket (framed)   ┌──────────────────────────────────────┐
 │ baton (TUI)  │◄───────────────────────►│ baton daemon                          │
 │ ratatui      │  control + PTY bytes    │                                       │
 │ vt100 mirror │                         │  Session ×N                           │
 │ tui-term     │                         │   ├─ portable-pty child: claude …     │
 └──────────────┘                         │   ├─ vt100::Parser (authoritative     │
                                          │   │   screen + scrollback)            │
 ┌──────────────┐  same socket, one-shot  │   ├─ status state machine             │
 │ baton hook   │────────────────────────►│   └─ transcript tailer (usage/cost)   │
 │ (run by      │  HookEvent{session,…}   │                                       │
 │  claude hook)│                         │  state.json (persisted metadata)      │
 └──────────────┘                         └──────────────────────────────────────┘
```

One binary, `baton`, with subcommands:
- `baton`: TUI. Connects to the daemon socket, auto-spawning the daemon (setsid, detached) if absent.
- `baton daemon [start|stop|status]`
- `baton hook <event>`: invoked by Claude Code hooks. Reads hook JSON on stdin, forwards it to the daemon, exits 0 within milliseconds with no stdout. **Never blocks Claude**: if the daemon is unreachable it silently exits 0.

### Daemon

- **tokio** runtime. One task per session handles PTY read → `vt100::Parser::process` → fan-out to the attached client. One task per client connection.
- **Spawning a session**: `portable-pty` spawns the profile command with
  `--settings <runtime_dir>/hooks.json` + resume flags + repo args,
  `cwd = repo`, env = profile env + `BATON_SESSION=<baton-id>` + `BATON_SOCK=<path>`.
- **Hooks are injected, not installed.** Baton generates `hooks.json` registering `baton hook <event>` for `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Notification`, `Stop`, `SessionEnd`. The user's `~/.claude*/settings.json` files are never modified, and both profiles work the same way.
- **Status state machine** (per session):
  - spawn → `starting`
  - `SessionStart` → `idle`; record `session_id` and `transcript_path`
  - `UserPromptSubmit` / `PreToolUse` / `PostToolUse` → `running`
  - `Notification` of type permission prompt → `permission`
  - `Stop` → `your-turn` (unseen) · client reports "viewed" → `idle`
  - `Notification` of type idle prompt → keep `your-turn`/`idle`
  - child exit (waitpid) → `exited(code)`
  - live process with no hook event for N s after spawn → `unknown`
- **Attach protocol**: on `Attach`, the daemon sends each session's `vt100` `state_formatted()` snapshot (screen + modes), then streams raw PTY bytes. The client feeds the same bytes into its own `vt100::Parser` mirror. Scrollback is fetched on demand (`GetScrollback{session, range}`) rather than replicated.
- **Resize**: the client sends `Resize{session, rows, cols}` when the panel size changes. The daemon resizes PTY + parser. All sessions are sized to the main panel, including background ones, so switching never reflows.
- **Usage tracking**: tail `transcript_path` (JSONL, from `SessionStart`) with `notify`. Sum `usage` from assistant entries. Context % = last turn's (input + cache_read + cache_creation) / model context window. Model id comes from the transcript.
- **Persistence** (`~/.local/state/baton/state.json`): per session `{project, repo, profile, claude_session_id, transcript_path, last_status}`. PTYs can't survive a daemon death. After a reboot or crash, opening a project relaunches with `--resume <claude_session_id>`, so the conversation continues.
- **Logging**: `tracing` to `~/.local/state/baton/daemon.log`.

### IPC

- Unix socket at `$XDG_RUNTIME_DIR/baton/baton.sock` (mode 0600).
- Frames: `tokio-util` `LengthDelimitedCodec` + `postcard` (serde) enums.
  - Client → daemon: `Attach`, `OpenProject`, `Restart`, `Input{session, bytes}`, `Resize`, `MarkViewed`, `GetScrollback`, `Detach`
  - Daemon → client: `Snapshot`, `Output{session, bytes}`, `StatusChanged`, `UsageUpdated`, `SessionList`, `Error`
  - Hook → daemon: `Hook{baton_session, event, payload_json}`
- Protocol version in the handshake. A mismatch (after upgrading the binary) prompts a daemon restart.

### TUI

- **ratatui** + **crossterm**. A single event loop over crossterm events, daemon messages and a render tick (render on change, capped at ~60 fps).
- Main panel: `tui-term`'s `PseudoTerminal` widget over the local `vt100` mirror.
- **Input encoding**: crossterm `KeyEvent` → terminal byte sequences (xterm encoding, respecting the app's DECCKM / bracketed-paste modes read from the mirror). Bracketed paste is forwarded intact. This encoder is hand-written and tested. It is the piece most likely to have bugs (see risks).
- Terminal focus tracking (crossterm `FocusGained` / `FocusLost`) feeds the notification rule.
- Notifications are sent **by the daemon** (`notify-rust`), so they fire even when no TUI is attached. The client reports what is on screen and whether the terminal is focused, so the daemon can suppress notifications for the session you're already looking at.

### Language decision: **Rust**

Live embedded interactive sessions are the core feature. Rust has the complete, mature stack for that (`portable-pty` + `vt100` + `tui-term` + `ratatui`), and the daemon benefits from tokio's async PTY/socket handling. Go/Bubble Tea would fit a dashboard with full-screen handoff, but its terminal-emulation-in-a-widget story is much weaker.

### Key crates

| Concern | Crate |
|---|---|
| TUI | `ratatui`, `crossterm` |
| Terminal widget | `tui-term` |
| Terminal emulation | `vt100` (fallback option: `alacritty_terminal` if fidelity issues) |
| PTY | `portable-pty` |
| Async / IPC | `tokio`, `tokio-util` (codec), `postcard`, `serde` |
| Config | `toml`, `serde`, `directories` / `xdg`, `shellexpand` |
| CLI | `clap` |
| Notifications | `notify-rust` |
| Transcript tailing | `notify`, `serde_json` |
| Errors / logs | `anyhow`, `thiserror`, `tracing`, `tracing-appender` |

Cargo workspace: `crates/baton-proto` (IPC types, protocol version), `crates/baton-core` (config, status machine, transcript parsing; no IO-heavy deps, unit-testable), `crates/baton` (binary: tui, daemon, hook subcommands).

## 7. Milestones

| # | Milestone | Done when |
|---|---|---|
| M0 | **Embedding spike** (in-process, no daemon) | One `claude` in a ratatui panel. Typing, permission prompts, Shift-Enter / multi-line, paste, resize and colors all behave like a bare terminal. *Go/no-go for vt100.* |
| M1 | Daemon + attach/detach | Quit and relaunch the TUI: the session is still running and the screen is intact. |
| M2 | Config, projects, profiles | Opening `loop` starts 3 sessions with the right profile/env. Session switching works. |
| M3 | Hooks → status, attention, notifications | Badges are accurate. `n` cycles attention sessions. Desktop notification on permission. |
| M4 | Restart / resume + persistence | Kill the daemon and reopen the project: conversations continue via `--resume`. |
| M5 | Session info panel | Context %, tokens and est. cost update live. |
| M6 | Keybinding config, help overlay, polish | Remapped keys work in both modes. |

## 8. Open risks

1. **Emulation fidelity** (highest). Claude Code's Ink UI uses synchronized output, wide/emoji glyphs, frequent full redraws and possibly the kitty keyboard protocol. `vt100` may mis-render, flicker or lack features. *Mitigation:* M0 spike first. Fall back to `alacritty_terminal` (heavier, more complete) behind a `Screen` trait.
2. **Key encoding.** Shift-Enter, Alt combos, Ctrl-chars and kitty-protocol negotiation must round-trip correctly, or multi-line prompts and shortcuts break. *Mitigation:* a dedicated encoder module with table tests. `Ctrl-\` and the `Alt-` chords must not collide with keys Claude uses.
3. **Hook contract drift.** Event names, `Notification` subtypes (permission vs idle) and `--settings` merging with user hooks must be verified against the installed Claude Code version. *Mitigation:* verify in M3, add `baton doctor` to check hooks fire, and show an `unknown` status rather than a wrong one.
4. **Transcript format is internal.** The JSONL schema can change between Claude Code releases. *Mitigation:* tolerant parsing. The info panel degrades to "n/a" without affecting anything else.
5. **Daemon death kills sessions** (PTY master closes → SIGHUP). *Mitigation:* `--resume` recovery (M4). Keep the daemon small and panic-free (no `unwrap` in session tasks, supervised tasks).
6. **Snapshot-on-attach accuracy.** `state_formatted()` may not restore every mode (mouse, kitty keys, alt-screen). *Mitigation:* test in M1. Optionally send a SIGWINCH nudge so Claude redraws itself after attach.
7. **Cost is an estimate.** Pricing changes, and subscription users aren't billed per token. Always labeled "est.". The price table lives in config.
8. **Two profiles, two transcript roots.** Transcript paths must come from the `SessionStart` hook, not be guessed from `~/.claude`.

## 9. Later versions (candidates)

- **v2:** quick-approve permission popup (blocking `PreToolUse` hook answered from the sidebar) · history picker (browse past conversations per repo and resume one) · ad-hoc sessions and rename/close · theming · tiled view of all project sessions.
- **v3:** on-demand worktree sessions for parallel work in one repo · broadcast prompt to project sessions · macOS support.
