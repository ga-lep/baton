# Baton MVP Plan
Status: APPROVED

## Goal
Build the Baton v1 MVP described in `docs/SPEC.md` §3: a Linux-only, lazygit-style Rust/Ratatui TUI (`baton`) backed by a background daemon (`baton daemon`). The daemon owns one PTY-embedded interactive `claude` session per configured repo. It tracks each session's status from injected Claude Code hooks (`baton hook <event>`) plus process supervision, keeps sessions alive across TUI restarts, resumes conversations after daemon death, tails transcripts for usage and estimated cost, and sends desktop notifications when a session needs attention. Tasks follow the spec's milestones M0–M6, and the embedding spike (M0) comes before any daemon work because emulation fidelity is the top risk. Every task keeps this gate green:

```
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
```

## Verified findings (Claude Code 2.1.295, checked 2026-10-08 on this machine)

Checked against `claude --help`, https://code.claude.com/docs/en/hooks and live runs under a PTY (`script`). The live runs used a `--settings` file whose hooks append their stdin JSON to a log.

1. **`--settings <file-or-json>`** loads additional settings. **Hooks from `--settings` are additive.** A `SessionStart` hook in `<repo>/.claude/settings.local.json` and one from `--settings` both fired in the same run, so Baton never needs to touch the user's `settings.json`. The hook command inherits the `claude` process env: `BATON_SESSION=test123` set on `claude` was visible inside the hook.
2. **Event names used by Baton** (all verified to fire live except `StopFailure`, which is documented only): `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `Notification`, `Stop`, `StopFailure`, `SessionEnd`. The docs list 33 events in total.
3. **Live sequence with a permission prompt** (`--permission-mode default`): `SessionStart(source=startup)` → `UserPromptSubmit` → `PreToolUse(tool_name=Bash)` → `PermissionRequest(tool_name=Bash)` → `Notification(notification_type="permission_prompt", message="Claude needs your permission")`.
   **Live sequence for a normal turn:** `SessionStart` → `UserPromptSubmit` → `PreToolUse` → `PostToolUse` → `Stop` → (later) `Notification(notification_type="idle_prompt", message="Claude is waiting for your input")`.
   - `PermissionRequest` fires first and is the earliest permission signal. Baton maps both `PermissionRequest` and `Notification/permission_prompt` to `permission`.
   - `PermissionRequest` hooks can return an allow/deny decision on stdout. Printing nothing makes no decision, which is one more reason the hook must stay silent.
4. **Notification subtypes** (field `notification_type`, also usable as matcher): `permission_prompt`, `idle_prompt`, `auth_success`, `elicitation_dialog`, `elicitation_url_dialog`, `elicitation_complete`, `elicitation_response`, `agent_needs_input`, `agent_completed`, `quota_auto_resume_*`.
   - Baton maps `permission_prompt` and `elicitation_dialog` / `elicitation_url_dialog` to `permission`, and `idle_prompt` to no change.
   - Any other or unknown subtype is ignored and logged.
5. **Common hook input fields:** `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `permission_mode`, `scratchpad_dir` (and others).
   - `SessionStart` adds `source` (`startup|resume|clear|compact|fork`) and `model`, e.g. `"claude-opus-5-5"`.
   - `SessionEnd` adds `reason` (`clear|resume|logout|prompt_input_exit|other`; a SIGTERM produced `other`).
   - `Stop` adds `stop_hook_active` and `last_assistant_message`.
   - `StopFailure` adds `error`.
   - `/clear` produces a new `session_id`, so the daemon must update `session_id` / `transcript_path` on **every** `SessionStart`.
6. **Hook stdout is dangerous:** for `SessionStart` and `UserPromptSubmit`, stdout on exit 0 is injected into Claude's context, and exit 2 blocks the prompt. `baton hook` must print nothing and always exit 0. Hooks are synchronous by default, with a 600 s default timeout. Baton registers them synchronously (ordering is preserved and the hook takes milliseconds) with `"timeout": 5`.
7. **Settings JSON shape:** `{"hooks":{"<Event>":[{"matcher":"…optional…","hooks":[{"type":"command","command":"…","timeout":5}]}]}}`.
8. **Resume behavior:**
   - `claude --resume <unknown-uuid>` prints `No conversation found with session ID: <id>` and exits with code 1 after about 2 s.
   - `--session-id <uuid>` exists, so Baton can assign the conversation id itself on fresh launches.
   - In an untrusted folder, Claude first shows a "trust this folder" dialog, and no hook fires until it is answered. The `unknown` timeout must not kill or relaunch anything.
9. **Terminal behavior of Claude's UI** (escape sequences captured from a live run):
   - Synchronized output `CSI ?2026h/l` (134 pairs in a short run).
   - Bracketed paste `?2004`, focus reporting `?1004`, `?2031`, and the alt screen `?1049h` with SGR mouse tracking `?1000/1002/1003/1006`. The user's work profile sets `"tui": "fullscreen"`.
   - Kitty keyboard negotiation: it pops `CSI <u`, pushes `CSI >5u`, and queries `CSI ?u`. It also sets xterm modifyOtherKeys `CSI >4;2m`.
   - Queries: XTVERSION `CSI >0q`, `OSC 7501;?`, then **DA1 `CSI c` as the sentinel**.
   - OSC 0 (title) and OSC 99 (kitty notification).

   Consequences:
   - The daemon must **answer DA1 (and DSR `CSI 6n`) itself**. `vt100` does not, and this must work even with no client attached.
   - Baton does **not** answer the kitty query, so Claude falls back to legacy / modifyOtherKeys input. The encoder must honour the modifyOtherKeys level it scans from the output stream.
   - Mouse wheel must be forwarded as SGR mouse when the app has enabled mouse mode. In fullscreen/alt-screen mode, Claude handles its own scrolling and Baton's local scrollback is empty.
10. **Transcript JSONL** (`~/.claude*/projects/<slug>/<session_id>.jsonl`):
    - Entry `type`s include `assistant`, `user`, `attachment`, `system`, `mode`, `permission-mode`, `last-prompt`, `ai-title`, `file-history-*`, among others.
    - `assistant` entries carry `message.model`, `message.id`, `isSidechain`, and `message.usage{input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens, …}`.
    - **One API message is split across several entries that repeat the same `message.id` and usage** (21 unique ids across 54 entries in a sample), so usage must be deduplicated by `message.id`.
    - The transcript file **may not exist yet** when `SessionStart` fires. In the test runs it was never created for sessions killed early, so the tailer must tolerate a missing file.
11. **Environment:**
    - `tmux` is **not installed**.
    - Available: `script`, `setsid`, `notify-send`, `dbus-monitor`, `jq`, Rust 1.97.1 with clippy and rustfmt.
    - Because tmux is missing, Task 3 builds a `baton-drive` PTY driver (a headless vt100 screen plus scripted keys and screen dumps) as the evidence surface. tmux `capture-pane` is an optional alternative if someone installs it.
    - `CLAUDE_CONFIG_DIR` in this shell points to `~/.claude-personal`.

## Conventions (apply to every task)
- Workspace layout:
  - `crates/baton-proto`: IPC types and protocol version.
  - `crates/baton-core`: pure logic (config, status machine, transcript parsing, terminal byte scanner/responder, notification rule, keymap parsing); no tokio and no PTY.
  - `crates/baton`: the binary (TUI, daemon, hook, debug, doctor).
  - `crates/baton-testkit`: `publish = false`; the `baton-drive` and `fake-claude` binaries plus a library used by integration tests.
- Edition 2024. Workspace lints: `clippy::unwrap_used = "deny"` and `clippy::expect_used = "deny"` for non-test code (`clippy.toml`: `allow-unwrap-in-tests = true`, `allow-expect-in-tests = true`). This enforces the spec's "no unwrap in session tasks".
- Crate versions (current on crates.io): `ratatui 0.30`, `crossterm 0.29`, `tui-term 0.3`, `vt100 0.16` (must match `tui-term`'s vt100; check with `cargo tree -i vt100` showing one version), `portable-pty 0.9`, `tokio 1`, `tokio-util 0.7` (codec), `postcard 1` (`alloc`), `serde`, `serde_json`, `toml`, `directories 6`, `shellexpand 3`, `clap 4` (derive), `notify-rust 4`, `uuid` (v4), `nix` (signal, process, fs, term), `shell-words`, `tracing`, `tracing-subscriber`, `tracing-appender`, `anyhow`, `thiserror`, `tempfile` (dev).
- **Deviation from spec:** transcript tailing uses a 1 s polling tailer instead of the `notify` crate. The latest `notify` is a 9.0 release candidate, and polling handles a not-yet-existing file, truncation and path changes with less code.
- **Test isolation:** env overrides `BATON_CONFIG` (config file path), `BATON_STATE_DIR` (default `~/.local/state/baton`) and `BATON_RUNTIME_DIR` (default `$XDG_RUNTIME_DIR/baton`). Every integration test and evidence command uses temp dirs through these. Tests never run real `claude` and never use D-Bus.
- **Finding test binaries:** integration tests locate workspace binaries (`baton`, `fake-claude`, `baton-drive`) with `baton_testkit::bin_path(name)`. It looks in the target dir derived from `current_exe()`, and if a binary is missing it runs `cargo build --bins -p <pkg>` once. `baton-testkit` keeps at least one integration test, so a plain `cargo test` at the workspace root builds its binaries.
- In examples, `D` is a scratch dir, and `export BATON_CONFIG=$D/config.toml BATON_STATE_DIR=$D/state BATON_RUNTIME_DIR=$D/run` is assumed before each Evidence command. `drive` means `target/debug/baton-drive`.

### Task 1: Workspace scaffold and CLI skeleton
Language: rust
Goal: Set up the Cargo workspace with four crates, lint policy and a clap CLI that has every subcommand stubbed, so later tasks only fill in bodies.
Files: `Cargo.toml` (workspace, `[workspace.lints]`, `[workspace.dependencies]`), `clippy.toml`, `rustfmt.toml`, `.gitignore`, `crates/baton-proto/{Cargo.toml,src/lib.rs}`, `crates/baton-core/{Cargo.toml,src/lib.rs}`, `crates/baton/{Cargo.toml,src/main.rs,src/cli.rs}`, `crates/baton-testkit/{Cargo.toml,src/lib.rs}`
Depends on: none
Acceptance criteria:
- The gate command passes on a clean checkout.
- `baton --help` lists `daemon`, `hook`, `spike`, `doctor`, `config`. A hidden `debug` subcommand exists but does not appear in `--help`.
- `baton daemon --help` lists `start`, `stop`, `status`. `baton hook --help` shows a positional `<EVENT>`.
- Unimplemented subcommands print `not implemented yet` to stderr and exit 2. The exception is `baton hook`, which always exits 0 with no output, even as a stub.
- A unit test asserts the clap command passes `debug_assert()`.
Evidence: `cargo run -q -p baton -- --help` lists the subcommands. `echo '{}' | cargo run -q -p baton -- hook Stop; echo $?` prints `0` and nothing else.

### Task 2: Terminal byte scanner and query responder (baton-core)
Language: rust
Goal: Add a pure, incremental scanner over PTY output bytes. It tracks the modes `vt100` does not expose: modifyOtherKeys level, kitty keyboard flag stack, synchronized-output state, focus-reporting `?1004`, mouse modes `?1000/1002/1003/1006`, and alt screen. It also produces replies to terminal queries: DA1 `CSI c`/`CSI 0c` → `ESC[?62;22c`, DSR `CSI 5n` → `ESC[0n`, and DSR `CSI 6n` → `ESC[<row>;<col>R` using a cursor position supplied by the caller. It deliberately does not answer `CSI ?u` (kitty), `CSI >0q` (XTVERSION) or `OSC 7501;?`.
Files: `crates/baton-core/src/term/mod.rs`, `crates/baton-core/src/term/scanner.rs`, `crates/baton-core/src/term/responder.rs`, `crates/baton-core/src/lib.rs`
Depends on: Task 1
Acceptance criteria:
- The `TermModes` struct exposes `modify_other_keys: u8`, `kitty_flags: u8` (top of the push/pop stack), `sync_output: bool`, `focus_reporting: bool`, `mouse: MouseMode`, `sgr_mouse: bool` and `alt_screen: bool`.
- Table tests use the exact byte string captured from Claude in Finding 9. After `ESC[<u ESC[>5u ESC[>4;2m … ESC[?2026h`, the modes are `kitty_flags=5`, `modify_other_keys=2` and `sync_output=true`. After `ESC[<u ESC[>4m ESC[?2026l`, they are `kitty_flags=0`, `modify_other_keys=0` and `sync_output=false`.
- Any escape sequence split at every possible byte boundary across two `feed()` calls produces the same modes and the same replies as a single feed (property-style loop test).
- `feed(b"\x1b[c")` yields exactly one DA1 reply. `\x1b[?u`, `\x1b[>0q` and `\x1b]7501;?\x1b\\` yield no reply.
- A `\x1b[6n` reply uses the 1-based cursor position passed in. Mode toggles inside OSC/DCS strings are not misparsed.
Evidence: none — pure library module, covered by unit tests (`cargo test -p baton-core term::`).

### Task 3: Test kit: `baton-drive` PTY driver and `fake-claude` stand-in
Language: rust
Goal: Provide the outside-in evidence surface, since tmux is absent, plus a cheap, deterministic stand-in for `claude` that speaks the verified hook contract.
- `baton-drive` runs a command in a PTY of a given size, keeps a headless `vt100` screen, and answers queries with the Task 2 responder (so crossterm's DA1 and DSR queries inside `baton` do not hang). It runs scripted steps and prints screen dumps.
- `fake-claude` mimics the parts of `claude` Baton relies on.
Files: `crates/baton-testkit/Cargo.toml`, `crates/baton-testkit/src/lib.rs` (`Drive` API, `bin_path`, `wait_for`), `crates/baton-testkit/src/bin/baton-drive.rs`, `crates/baton-testkit/src/bin/fake-claude.rs`, `crates/baton-testkit/tests/drive_smoke.rs`
Depends on: Task 2
Acceptance criteria:
- **`baton-drive` CLI:** `baton-drive [--size ROWSxCOLS] [--timeout-ms N] [--step STEP]... -- CMD [ARGS]...`.
  - STEP is one of `send:<text>`, `wait:<regex>`, `resize:ROWSxCOLS`, `sleep:<ms>`, `dump`, `expect-exit:<code>`.
  - In `<text>`, `\r \n \t \e \\ \xNN` are unescaped.
  - A `wait` that times out prints the final screen to stderr and exits 1. `dump` prints the screen rows, trimmed, between `--- screen ROWSxCOLS ---` markers.
- **`fake-claude` arguments:** parses `--settings <path>`, `--resume <id>`, `--continue`, `--session-id <uuid>` and `--model <m>`, and ignores unknown args. It appends one JSON line per launch to `$FAKE_CLAUDE_LOG` if set: argv, cwd, and the env vars `BATON_SESSION`, `BATON_SOCK` and `CLAUDE_CONFIG_DIR`.
- **`fake-claude` startup:**
  - It writes DA1 `ESC[c` and waits up to 2 s for a reply, then prints `DA1: ok` or `DA1: timeout`.
  - It prints `FAKE CLAUDE session=<id> model=<m>` and shows a `> ` prompt.
  - It writes its transcript to `$FAKE_CLAUDE_HOME/projects/<slug>/<id>.jsonl`. `FAKE_CLAUDE_HOME` defaults to `$CLAUDE_CONFIG_DIR` and then `~/.fake-claude`.
- **`fake-claude` hooks:** it reads the hooks file given by `--settings`. For each event it runs every registered command via `sh -c` with the event JSON on stdin (the common fields from Finding 5 plus event-specific fields). It fires `SessionStart` at startup with `source` set to `startup` or `resume`. `FAKE_CLAUDE_NO_HOOKS=1` disables all hook firing.
- **`fake-claude` resume rules:** `--resume <id>` with an unknown id (no transcript file) prints `No conversation found with session ID: <id>` and exits 1. `--continue` with no prior transcript for the cwd prints `No conversation found to continue` and exits 1. A fresh launch uses `--session-id` if given, otherwise a new v4 uuid.
- **`fake-claude` input lines:**

  | Line | Effect |
  |---|---|
  | `prompt <text>` | Fires `UserPromptSubmit`, `PreToolUse` and `PostToolUse`. Appends two `assistant` entries sharing one `message.id` with fixed usage (in=100, out=50, cache_read=1000, cache_creation=10, model from `--model`, default `claude-opus-5-5`). Then fires `Stop`. |
  | `perm` | Fires `PreToolUse`, `PermissionRequest`, then `Notification{notification_type:"permission_prompt"}`. |
  | `allow` | Fires `PostToolUse`, then `Stop`. |
  | `idle` | Fires `Notification{notification_type:"idle_prompt"}`. |
  | `color` | Prints red text with SGR. |
  | `exit <n>` | Fires `SessionEnd{reason:"prompt_input_exit"}` and exits with code n. |

- `drive_smoke.rs`:
  - Drives `bash --norc --noprofile` through `baton-drive`: `echo hi`, wait for `hi`, resize, and check that `tput cols` shows the new width.
  - Drives `fake-claude` with a temp `--settings` hooks file that appends stdin to a file, and asserts the `SessionStart` JSON has `hook_event_name`, `session_id` and `transcript_path`.
  - Asserts `DA1: ok` appears under `baton-drive`.
Evidence: `drive --size 24x80 --step 'wait:DA1: ok' --step 'send:prompt hi\r' --step 'wait:> ' --step dump -- target/debug/fake-claude` prints a screen containing `FAKE CLAUDE session=` and `DA1: ok`.

### Task 4: Key, paste, mouse and focus encoder (hand-written, table-tested)
Language: rust
Goal: Convert crossterm input events into the bytes a terminal application expects, honouring the target app's modes. This is Risk 2 in the spec, so it gets a dedicated module with exhaustive table tests.
Files: `crates/baton/src/term/mod.rs`, `crates/baton/src/term/encode.rs`, `crates/baton/src/term/encode_tests.rs`
Depends on: Task 2
Acceptance criteria:
- The API is `encode_key(&KeyEvent, &EncodeModes) -> Option<Vec<u8>>`, `encode_paste(&str, &EncodeModes) -> Vec<u8>`, `encode_mouse(&MouseEvent, origin, &EncodeModes) -> Option<Vec<u8>>` and `encode_focus(bool, &EncodeModes) -> Option<Vec<u8>>`.
- `EncodeModes` holds `app_cursor` (DECCKM), `app_keypad`, `bracketed_paste`, and the `TermModes` from Task 2.
- The table tests have at least 60 rows and cover these cases:

  | Input | Expected bytes |
  |---|---|
  | Printable ASCII and UTF-8 (`é`, CJK, emoji) | The UTF-8 bytes |
  | Enter | `\r` |
  | Tab | `\t` |
  | Backspace | `\x7f` |
  | Esc | `\x1b` |
  | Ctrl-a..z | `0x01..0x1a` |
  | Ctrl-@ / Ctrl-Space | `0x00` |
  | Ctrl-[ | `0x1b` |
  | Ctrl-] | `0x1d` |
  | Ctrl-^ | `0x1e` |
  | Ctrl-_ | `0x1f` |
  | Arrows, normal mode | `ESC[A` … |
  | Arrows, DECCKM on | `ESCOA` … |
  | Arrows with modifiers | `ESC[1;5C` … |
  | Home/End/PgUp/PgDn/Insert/Delete | xterm codes, with modifiers |
  | F1–F12 | xterm codes |
  | Alt-x | `ESC x` |
  | Alt-Enter | `ESC \r` |
  | BackTab | `ESC[Z` |

- **Shift-Enter:**
  - With modifyOtherKeys ≥ 1, it encodes as `ESC[27;2;13~`.
  - Otherwise it encodes as `ESC \r` (the meta-enter that Claude treats as a newline).
  - If `kitty_flags & 1`, it encodes as `ESC[13;2u`.
  - The M0 spike confirms which of these Claude accepts.
- With modifyOtherKeys = 2, Ctrl-<punct> and Ctrl-Enter use `ESC[27;<mod>;<code>~`.
- **Paste:**
  - Bracketed paste on wraps the text in `ESC[200~` … `ESC[201~`. Any embedded `ESC[201~` in the pasted text is stripped.
  - Bracketed paste off sends the text with `\n` converted to `\r`.
- **Mouse:** a wheel event encodes as SGR `ESC[<64;x;yM` / `ESC[<65;x;yM`, relative to the panel origin, only when mouse mode and SGR are on. Otherwise it returns `None`.
- **Focus:** `CSI I` / `CSI O` only when focus reporting is on.
- Key releases, and events in states the encoder ignores, return `None`.
Evidence: none — internal module, proven by table tests (`cargo test -p baton term::encode`). It is exercised from outside in Task 6.

### Task 5: `Screen` abstraction with a vt100 implementation
Language: rust
Goal: Hide the terminal emulator behind a `Screen` trait, the spec's mitigation for Risk 1. The daemon (authoritative), the TUI mirror and the spike all use it, so `alacritty_terminal` can replace `vt100` later without touching callers.
Files: `crates/baton/src/term/screen.rs`, `crates/baton/src/term/vt100_screen.rs`, `crates/baton/src/term/mod.rs`
Depends on: Task 2, Task 4
Acceptance criteria:
- The `Screen` trait (`Send`) has these methods:
  - `process(&mut self, bytes) -> Vec<u8>`, which returns the query replies from the Task 2 responder, with the DSR cursor taken from the screen.
  - `resize(rows, cols)`, `size()` and `cursor()`.
  - `encode_modes() -> EncodeModes`, which combines vt100's DECCKM/keypad/bracketed-paste with the Task 2 scanner modes.
  - `snapshot() -> Vec<u8>`: vt100 `state_formatted()` plus re-emitted scanner modes (mouse, focus, modifyOtherKeys, alt screen) so a fresh mirror reproduces them.
  - `scrollback_len()`, `scrollback_rows(start, count) -> Vec<Vec<u8>>` (formatted rows), `set_view_offset(n)` and `title()`.
  - `render(&self, area, &mut Buffer, show_cursor)` via `tui-term`'s `PseudoTerminal`.
- `Vt100Screen::new(rows, cols, scrollback_cap)` implements it.
- Test: feeding a captured byte stream into screen A, then feeding `A.snapshot()` into a fresh screen B, gives identical `contents()`, cursor position, `encode_modes()` and alt-screen flag. Cases include colored output, alt screen, mouse modes and modifyOtherKeys.
- Test: scrollback beyond the cap is trimmed to the cap, and `scrollback_rows` returns rows oldest-first.
- Test: rendering into a ratatui `TestBackend` shows the expected text and cell colors for an SGR-red line.
- `cargo tree -i vt100` resolves to a single version.
Evidence: none — internal abstraction, proven by unit tests. It is exercised from outside in Task 6.

### Task 6: M0 embedding spike (`baton spike`), the go/no-go for vt100
Language: rust
Goal: Embed one PTY child in a ratatui panel in-process (no daemon) with focus mode, resize, paste, colors and mouse wheel, and run the M0 go/no-go checklist against real Claude.
Files: `crates/baton/src/spike.rs`, `crates/baton/src/tui/terminal_guard.rs` (raw mode, alt screen, keyboard enhancement, bracketed paste, focus-change and mouse capture enable/disable, and terminal restore on panic), `crates/baton/src/tui/render_pacer.rs`, `crates/baton/tests/e2e_spike.rs`, `docs/m0-spike.md`
Depends on: Task 3, Task 4, Task 5
Acceptance criteria:
- **Command and layout:** `baton spike [-- CMD ARGS...]` defaults to `claude`. The layout is a 32-col sidebar placeholder, a bordered main panel and a bottom bar. The bar shows the mode (`FOCUS` / `NORMAL`) and the inner panel size as `<rows>x<cols>`.
- **Focus mode and keys:**
  - The spike starts in focus mode. `Ctrl-\` switches to normal mode and is not forwarded. The match accepts both `Char('\\')+CONTROL` and crossterm's legacy `Char('4')+CONTROL`.
  - In normal mode, `Enter` refocuses and `q` quits, killing the child.
  - Keys go through the Task 4 encoder with the screen's current `encode_modes()`.
  - Bracketed paste events are forwarded through `encode_paste`.
  - Mouse wheel over the panel is forwarded when the app enabled mouse mode. Otherwise it scrolls local scrollback.
- **Rendering:** PTY replies from `Screen::process` are written back to the PTY. A resize of the host terminal resizes the PTY and the screen to the inner panel size. The pacer renders on change at no more than 60 fps, and defers rendering while `sync_output` is on (until it ends, or for at most 50 ms).
- **Host terminal setup:** when `supports_keyboard_enhancement()` is true, the spike pushes `DISAMBIGUATE_ESCAPE_CODES | REPORT_ALTERNATE_KEYS`. Leaving restores the host terminal, including on panic.
- **e2e test:** `e2e_spike.rs` drives `baton spike -- bash --norc --noprofile` via `baton-drive` at 40x120. It checks:
  - The bar shows `37x86` (40 − 2 border − 1 bar = 37 rows; 120 − 32 − 2 = 86 cols).
  - `tput cols` prints `86`. After `resize:30x100`, `tput cols` prints `66`.
  - `printf '\e[31mRED\e[0m\n'` shows `RED`.
  - `\x1c` shows `NORMAL`, and `q` exits 0.
- **e2e test with `fake-claude`:** `DA1: ok` is shown, proving the spike answers queries.
- **Manual checklist:** `docs/m0-spike.md` records a checklist run against real `claude` (manual and optional in CI; uses no API tokens unless a prompt is sent). The items are: typing, slash-command menu, permission prompt, Shift-Enter newline (which encoding Claude accepted), multi-line paste, resize, colors/emoji, fullscreen-tui mouse scroll and flicker. It ends with a GO/NO-GO for vt100. A NO-GO inserts a task "AlacrittyScreen impl of Screen" before Task 7.
Evidence: `drive --size 40x120 --step 'wait:FOCUS' --step 'send:tput cols\r' --step 'wait:^86$' --step 'send:\x1c' --step 'wait:NORMAL' --step dump --step 'send:q' --step 'expect-exit:0' -- target/debug/baton spike -- bash --norc`. Optional with real claude: `drive --size 40x120 --step 'wait:(trust|❯)' --step dump -- target/debug/baton spike` (no prompt sent, so no token cost).

### Task 7: IPC protocol crate (`baton-proto`)
Language: rust
Goal: Define every frame exchanged between TUI, daemon and hook as serde enums with a protocol version, encoded as `postcard` inside `LengthDelimitedCodec` frames.
Files: `crates/baton-proto/Cargo.toml`, `crates/baton-proto/src/lib.rs`, `crates/baton-proto/src/codec.rs`
Depends on: Task 1
Acceptance criteria:
- **Constants and ids:** `PROTOCOL_VERSION: u32` and `MAX_FRAME: usize = 16 MiB`. `SessionId(String)` is the stable baton id `"<project>/<repo-path>"`, built from the expanded, canonical path.
- **`ClientMsg`:**
  - `Hello{version, role: Tui|Hook|Ctl}`, `Attach{rows, cols}`, `OpenProject{name}`, `Restart{session}`, `Input{session, bytes}`, `Resize{rows, cols}` (applies to all sessions, per spec §6).
  - `MarkViewed{session}`, `ClientView{on_screen: Option<SessionId>, terminal_focused: bool}`, `GetScrollback{session, start, count}`, `Detach`.
  - `Hook{baton_session, event, payload_json: String}`, `Status`, `Shutdown`.
- **`DaemonMsg`:**
  - `Welcome{version, pid}` and `VersionMismatch{daemon_version}`.
  - `SessionList(Vec<SessionInfo>)`, `Snapshot{session, rows, cols, bytes}`, `Output{session, bytes}`, `StatusChanged{session, status}`, `UsageUpdated{session, usage}`, `Scrollback{session, start, rows}`.
  - `DaemonStatus{pid, version, sessions}` and `Error{message}`.
- **Data types:**
  - `SessionInfo{id, project, repo, profile, status, claude_session_id, model, started_at, exit_code, usage}`.
  - `Status` is `Starting|Running|Permission|YourTurn|Idle|Exited(i32)|Unknown`.
  - `Usage{input, output, cache_read, cache_write, context_pct: Option<f32>, cost_usd: Option<f64>, model}`.
- **Codec:** `encode(&T) -> Bytes` and `decode(&[u8]) -> Result<T>` helpers, plus a `framed(stream)` helper over `tokio_util::codec::Framed`.
- **Tests:** a round-trip test for every variant. Decoding garbage returns `Err` and never panics. A frame over `MAX_FRAME` is rejected.
Evidence: none — protocol library, covered by round-trip tests (`cargo test -p baton-proto`).

### Task 8: Config and paths (`baton-core`), plus `baton config check`
Language: rust
Goal: Parse `config.toml` (spec §5) into a validated, resolved model: projects → sessions with effective profile, command, env and args. Resolve all paths with env overrides, and expose them through a diagnostic subcommand.
Files: `crates/baton-core/src/config.rs`, `crates/baton-core/src/paths.rs`, `crates/baton-core/src/pricing.rs` (types only), `crates/baton/src/cmd/config.rs`, `crates/baton-core/tests/config_examples.rs`
Depends on: Task 1
Acceptance criteria:
- The spec §5 example parses verbatim (test fixture).
- **Defaults:**
  - `editor = "xdg-open {path}"`, `notifications = true` and `scrollback_lines = 10000`.
  - An implicit profile `default` with `command = "claude"`.
  - `hook_timeout_secs = 20`, used by `unknown`.
- **Expansion:** `~` and `$VAR` are expanded in repo paths, profile `env` values and the profile `command`.
- **Resolution:** a repo's `profile` overrides the project's profile, which overrides `default`. `args` are appended after Baton's own flags. The profile `command` is split with `shell-words`, so `command = "bash --norc"` works.
- **Validation errors** (`thiserror`, with project/repo context): unknown profile, duplicate project name, duplicate repo path within a project, empty repos, and an invalid TOML key path.
- **Paths:** `paths::{config_file, state_dir, runtime_dir, socket_path, hooks_json_path, state_json_path, log_path}` honour `BATON_CONFIG`, `BATON_STATE_DIR` and `BATON_RUNTIME_DIR`. Defaults follow spec §6/§5 (XDG). A missing `XDG_RUNTIME_DIR` falls back to `/tmp/baton-<uid>` with mode 0700.
- **Missing config file:** the result is an empty config, not an error.
- **`baton config check`** prints one line per session (`<project>  <repo>  profile=<p>  cmd=<argv>  env=<keys>`) and exits 0. On invalid config it prints the error and exits 1.
Evidence: `printf '[profiles.p]\ncommand="bash --norc"\nenv={FOO="bar"}\n[[projects]]\nname="x"\nprofile="p"\nrepos=[{path="/tmp"}]\n' > $D/config.toml && baton config check` prints `x  /tmp  profile=p  cmd=["bash","--norc"]  env=FOO`. Changing the profile to `nope` makes it exit 1 with `unknown profile "nope"`.

### Task 9: Daemon lifecycle, socket, handshake, `baton daemon start|stop|status`
Language: rust
Goal: Run a tokio daemon that owns a 0600 unix socket and enforces a single instance. It handshakes on protocol version, logs to a file, shuts down cleanly, and can be spawned detached by a client helper.
Files: `crates/baton/src/daemon/mod.rs`, `crates/baton/src/daemon/server.rs`, `crates/baton/src/daemon/lifecycle.rs`, `crates/baton/src/client.rs` (connect, handshake, `ensure_daemon()` auto-spawn), `crates/baton/src/cmd/daemon.rs`, `crates/baton/src/logging.rs`, `crates/baton/tests/e2e_daemon.rs`
Depends on: Task 7, Task 8
Acceptance criteria:
- **Start:**
  - `baton daemon start` detaches: it re-execs itself with `--foreground` under `setsid`, with stdio set to `/dev/null`. It returns once the socket accepts a connection (5 s timeout), and prints `daemon started pid=<pid>`.
  - `--foreground` runs attached.
  - Starting while a daemon is running prints `daemon already running pid=<pid>` and exits 0.
- **Single instance and socket:** single instance is enforced with `flock` on `<runtime>/baton.lock`, and a stale socket file is removed only while the lock is held. The runtime dir has mode 0700 and the socket 0600 (test checks the permissions).
- **Handshake:** the first frame must be `Hello`. A version mismatch gets a `VersionMismatch` reply and the connection is closed. Any other first frame closes the connection.
- **Status:** `baton daemon status` prints `running pid=<pid> protocol=<v> sessions=<n>` and exits 0. If no daemon is running, it prints `not running` and exits 1.
- **Stop:** `baton daemon stop` sends `Shutdown`. On `Shutdown` or SIGTERM, the daemon kills child process groups (SIGHUP, then SIGKILL after 3 s), removes the socket and exits 0. `stop` waits up to 5 s for the socket to disappear.
- **Robustness and logs:** a client disconnecting mid-frame does not crash the daemon. Logs go to `<state>/daemon.log` via `tracing-appender`.
- **`client::ensure_daemon()`:** connects, or else spawns `baton daemon start` and retries with backoff for up to 5 s.
- **e2e test:** start → status → second start says already running → stop → status says not running, all in temp dirs.
Evidence: `baton daemon start && baton daemon status && stat -c %a $D/run/baton.sock && baton daemon stop && baton daemon status; echo $?` prints `daemon started pid=…`, `running pid=… protocol=1 sessions=0`, `600`, `not running` and `1`.

### Task 10: Session runtime in the daemon (PTY, authoritative screen, attach/stream) and the `baton debug` client
Language: rust
Goal: Implement `OpenProject` → spawn one PTY session per repo with the resolved profile. Feed PTY output through the authoritative `Screen` (answering queries even with no client attached), and fan out to the attached client. Implement Attach (SessionList + Snapshots + stream), Input, Resize, GetScrollback and Detach, and supervise child exit. Add a hidden `baton debug` client as an evidence surface that does not need the TUI.
Files: `crates/baton/src/daemon/session.rs`, `crates/baton/src/daemon/registry.rs`, `crates/baton/src/daemon/spawn.rs`, `crates/baton/src/daemon/server.rs`, `crates/baton/src/cmd/debug.rs`, `crates/baton/tests/e2e_sessions.rs`
Depends on: Task 5, Task 9
Acceptance criteria:
- **`OpenProject{name}`:**
  - Re-reads config and spawns sessions only for repos not already live.
  - Spawn uses `portable-pty` with the profile argv plus repo `args`, `cwd = repo`, and env = inherited + profile env + `BATON_SESSION=<id>` + `BATON_SOCK=<socket>`.
  - The child is in its own process group/session.
  - An unknown project returns `Error`.
  - Launch flags (`--settings`, resume) come in Tasks 13 and 16.
- **PTY reader:** a blocking reader thread per session sends chunks over a tokio mpsc channel to the session task. The session task calls `screen.process()`, writes any replies back to the PTY, and forwards `Output` to the attached client. No `unwrap`/`expect`; errors are logged.
- **Child exit:** detected via a wait thread, it sets `Exited(code)` and emits `StatusChanged`. The session stays listed.
- **Attach:**
  - The daemon sends `SessionList`, then one `Snapshot` per live session, then streams `Output`.
  - Only one client may be attached; a second `Attach` takes over and the previous client gets `Error{"replaced by another client"}`.
  - After a snapshot, the daemon nudges a redraw with a resize to `cols-1` then `cols`, which sends SIGWINCH (Risk 6). This is configurable as `attach_redraw_nudge = true`.
- **Resize:** resizes all sessions (PTY and screen) and is a no-op if the size is unchanged.
- **Scrollback:** `GetScrollback` returns formatted rows from the authoritative screen.
- **`baton debug` (hidden):**
  - `open <project>`.
  - `sessions [--json]` (prints the SessionList).
  - `send <session> <text>` (with the same escapes as `baton-drive`).
  - `screen <session> [--rows R --cols C]`: attaches, feeds the snapshot plus 300 ms of output into a local mirror `Screen`, and prints its text.
  - `scrollback <session> <start> <count>`.
- **e2e test** with profile `bash --norc --noprofile`:
  - Open a 2-repo project. `send` `echo $BATON_SESSION; echo $FOO; pwd` shows the baton id, the profile env and the repo path in `debug screen`.
  - Disconnect and reattach: the screen text is identical.
  - Run `exit 3`: `sessions --json` shows `Exited(3)`.
  - With `fake-claude` as the profile command, `debug screen` shows `DA1: ok` while no client was attached during startup.
Evidence: config with project `x` (repos `/tmp`, `$HOME`; profile command `bash --norc`). `baton debug open x && baton debug send 'x//tmp' 'echo marker-$BATON_SESSION\r' && baton debug screen 'x//tmp'` prints a line `marker-x//tmp`. `baton debug sessions` lists 2 sessions with status `Starting`. `Unknown` only applies after Task 14.

### Task 11: TUI client M1: attach, embedded main panel, focus mode, quit and reattach
Language: rust
Goal: Make `baton` (no subcommand) a ratatui client that ensures and attaches to the daemon, mirrors the selected session through a local `Screen` fed by Snapshot + Output, and renders it in the main panel. It also supports focus mode, quitting without stopping sessions, and handling a version mismatch.
Files: `crates/baton/src/tui/mod.rs`, `crates/baton/src/tui/app.rs` (state + reducer), `crates/baton/src/tui/event_loop.rs`, `crates/baton/src/tui/ui.rs`, `crates/baton/src/tui/mirror.rs`, reuse `tui/terminal_guard.rs` and `tui/render_pacer.rs`, `crates/baton/tests/e2e_tui_attach.rs`
Depends on: Task 6, Task 10
Acceptance criteria:
- **Event loop:** a single `tokio::select!` over crossterm `EventStream`, daemon frames and the render pacer tick. It renders only on change, at no more than 60 fps, and defers rendering during a mirror's `sync_output`.
- **Mirrors:** one per session, created from `Snapshot` and fed `Output`. The mirror **never** writes query replies back (the daemon answers). A unit test asserts that `Output` containing `ESC[c` produces no `Input` frame.
- **Layout:** spec §4 layout. The sidebar shows a flat session list (the project tree comes in Task 12), plus a placeholder info panel, the main panel, and the bottom bar text from spec §4 for NORMAL / FOCUS. In focus mode, the main panel border is highlighted.
- **Keys:**
  - `Enter` focuses the selected session. In focus mode, every key and paste goes through the encoder to `Input{session}`; `Ctrl-\` (both crossterm forms) returns to normal mode.
  - Focus in/out bytes are sent if the app enabled `?1004`.
  - Mouse wheel is forwarded per Task 4.
- **Resize:** a host terminal resize sends `Resize{rows, cols}` with the main panel's inner size.
- **Quit:** `q` in normal mode sends `Detach`, restores the terminal and exits 0. The daemon and sessions keep running.
- **Version mismatch:** the client shows a modal `Daemon protocol v<a> ≠ v<b>. Restart daemon (sessions will be resumed)? [y/N]`. `y` runs stop + start, then attaches.
- **Daemon gone:** if the daemon disappears while attached, the client shows `daemon disconnected — press r to reconnect, q to quit` and does not panic.
- **e2e test:** the Evidence flow below, automated.
Evidence: a project of one `bash --norc` repo, opened with `baton debug open x`.
1. `drive --size 40x120 --step 'wait:NORMAL' --step 'send:\r' --step 'wait:FOCUS' --step 'send:echo persisted-42\r' --step 'wait:persisted-42' --step 'send:\x1c' --step 'send:q' --step 'expect-exit:0' -- target/debug/baton`
2. `baton daemon status` → `sessions=1`.
3. `drive --size 40x120 --step 'wait:persisted-42' --step dump -- target/debug/baton` shows the screen intact after relaunch (M1 done).

### Task 12: TUI M2: project tree sidebar, open project, session switching, scrollback
Language: rust
Goal: Show all configured projects in the sidebar (closed ones dimmed), open projects from the TUI, switch between sessions and view scrollback.
Files: `crates/baton/src/tui/sidebar.rs`, `crates/baton/src/tui/app.rs`, `crates/baton/src/tui/ui.rs`, `crates/baton/src/tui/scrollback.rs`, `crates/baton/tests/e2e_tui_projects.rs`
Depends on: Task 11
Acceptance criteria:
- **Sidebar:** the client reads config on attach and merges it with the `SessionList`. Each project row is `▾ name` (open) or `▸ name  (closed)`, dimmed when it has no live sessions. Sessions are listed under their project as `N <badge> <repo-name>  <status text>`. Unit-tested with ratatui `TestBackend`.
- **Normal-mode keys:**
  - `j`/`k`/`↓`/`↑` move over projects and sessions.
  - `Enter`/`l` on a session focuses it; on a closed project, it sends `OpenProject`.
  - `o` opens the project under the cursor.
  - `1`..`9` selects session N of the current project.
- **Switching:** the selected session shows in the main panel immediately, without reflow, because all sessions share the panel size. The main panel title is `<repo> · <profile> · <badge> <status>`.
- **Scrollback:**
  - `Ctrl-u`/`Ctrl-d`/`PgUp`/`PgDn` scroll the pane by half or full pages using `GetScrollback` pages, which are cached per session. A `[scrollback -N]` indicator shows in the title.
  - `G` returns to live view.
  - Scrolling is disabled with a hint `app manages its own scrolling (mouse wheel)` when the session is in the alt screen.
- **Info panel (static fields):** repo path, profile, status, uptime.
- **e2e test:** a project with 3 repos and profiles `a` (`bash --norc`, env `WHO=a`) and `b` (env `WHO=b`, per-repo override). Open it with `o`, and in each session `echo $WHO` shows the right value. `2` / `3` switch sessions and the panel content changes. After `seq 1 500`, `Ctrl-u` shows earlier numbers and `G` returns to live.
Evidence: `drive --size 40x120 --step 'wait:▸ loop' --step 'send:o' --step 'wait:3 . hyper' --step 'send:3' --step 'send:\r' --step 'send:echo WHO=$WHO\r' --step 'wait:WHO=b' --step dump -- target/debug/baton` shows the 3 sessions under `▾ loop` and the per-repo profile env (M2 done).

### Task 13: Hook plumbing: injected `hooks.json`, `--settings` launch flag and `baton hook`
Language: rust
Goal: The daemon generates `<runtime>/hooks.json` registering `baton hook <Event>` for the events in Finding 2, and launches every session with `--settings <path>`. `baton hook` reads stdin and forwards a `Hook` frame. It never blocks, never prints, and always exits 0.
Files: `crates/baton-core/src/hooks.rs` (settings JSON generation + payload parsing of the fields from Finding 5), `crates/baton/src/cmd/hook.rs`, `crates/baton/src/daemon/spawn.rs`, `crates/baton/src/daemon/session.rs`, `crates/baton/tests/e2e_hook.rs`
Depends on: Task 10
Acceptance criteria:
- **`hooks.json` generation:**
  - Written atomically at daemon start. It registers 9 events (`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `Notification`, `Stop`, `StopFailure`, `SessionEnd`), each `[{"hooks":[{"type":"command","command":"'<abs current_exe>' hook <Event>","timeout":5}]}]`, with no matcher. The exe path is shell-quoted.
  - A golden-file unit test checks the JSON.
- **Spawn argv:** profile argv + `--settings <hooks.json>` + (resume flags, Task 16) + repo args.
- **`baton hook <event>` behavior:**
  - **Exits 0** in all cases: daemon down, `BATON_SESSION` unset, malformed or empty stdin, stdin over 1 MiB (truncated), slow stdin (500 ms read deadline), socket path pointing at a non-socket.
  - **Writes nothing to stdout**. Stderr is written only if `BATON_HOOK_DEBUG=1`.
  - Connects with a 200 ms timeout, sends `Hello{role: Hook}` then `Hook{…}`, and does not wait for a reply. Total wall time is under 300 ms even when the daemon is unreachable (asserted in tests with a generous bound).
- **Daemon handling:** the daemon parses the payload tolerantly (unknown fields ignored, missing fields → `None`). On `SessionStart` it records `claude_session_id`, `transcript_path` and `model` on the session (updating them on every `SessionStart`, Finding 5). It logs `hook <event> session=<id>` at debug level. A hook for an unknown `BATON_SESSION` is logged and dropped.
- **e2e test** with the `fake-claude` profile: after open, `debug sessions --json` shows `claude_session_id` equal to the id `fake-claude` printed on screen, and a `transcript_path` under `$FAKE_CLAUDE_HOME`.
Evidence:
- `echo '{"hook_event_name":"Stop"}' | BATON_SESSION=x BATON_SOCK=/nonexistent/sock baton hook Stop; echo "rc=$?"` prints only `rc=0`. `time` shows under 0.3 s. The same with `printf 'garbage'` gives the same result.
- With the `fake-claude` profile: `baton debug open x && sleep 1 && baton debug sessions --json | jq '.[0].claude_session_id'` returns a uuid.
- Optional with real claude (no tokens; SessionStart fires at startup): a profile with `command="claude"` on a trusted repo shows the real session id in `debug sessions`.

### Task 14: Status state machine, attention and `n` / `Alt-n` (M3a)
Language: rust
Goal: Implement the per-session status machine from spec §6, refined by Findings 3–5, as a pure function in `baton-core`. Wire it into the daemon with the `unknown` timer and view tracking, render badges, and implement next-attention navigation.
Files: `crates/baton-core/src/status.rs`, `crates/baton-core/src/attention.rs`, `crates/baton/src/daemon/session.rs`, `crates/baton/src/daemon/server.rs`, `crates/baton/src/tui/app.rs`, `crates/baton/src/tui/sidebar.rs`, `crates/baton/tests/e2e_status.rs`
Depends on: Task 12, Task 13
Acceptance criteria:
- `status::next(current, Input) -> (Status, Effects)` is pure. Its table tests cover every row below, plus "Exited is terminal until relaunch":

  | Input | Result |
  |---|---|
  | spawn | `Starting` |
  | `SessionStart` | `Idle` (from Starting or Unknown) |
  | `UserPromptSubmit` / `PreToolUse` / `PostToolUse` | `Running` |
  | `PermissionRequest`, or `Notification{permission_prompt \| elicitation_dialog \| elicitation_url_dialog}` | `Permission` |
  | `Stop` / `StopFailure` | `YourTurn` |
  | `Notification{idle_prompt}` | no change |
  | Unknown event or subtype | no change |
  | `SessionEnd` | no change (the process exit decides) |
  | viewed while in `YourTurn` | `Idle` |
  | child exit | `Exited(code)` |
  | `hook_timeout_secs` elapsed in `Starting` with the process alive | `Unknown` |
  | any hook while `Unknown` | applied normally |

- **Attention:** `Permission` or `YourTurn`. `attention::next_after(order, current)` returns the next attention session in sidebar order, wrapping around and excluding the current one. Unit-tested.
- **Viewed and client view:**
  - The client sends `ClientView{on_screen, terminal_focused}` whenever the selected session or the focus changes.
  - The daemon treats a session as viewed (YourTurn → Idle) when it is `on_screen` of an attached client. This also covers a `Stop` that arrives while you are watching.
  - `MarkViewed` forces it.
  - Every transition emits `StatusChanged`.
- **TUI badges** follow spec §4: `…` starting, `●` running, `◐` permission, `✓` your turn, `○` idle, `✗` exited, `?` unknown. Badges for sessions needing attention are highlighted.
- **Next attention:** `n` (normal mode) and `Alt-n` (focus mode, which stays in focus mode) jump to the next attention session. `Alt-1`..`Alt-9` switch sessions in focus mode. If no session needs attention, the bottom bar shows `no session needs attention`.
- **e2e test** with `fake-claude`: send `perm` and the session shows `◐ permission`. Send `allow` → `✓ your turn` while another session is on screen. `n` jumps to it and its badge becomes `○ idle`. With `FAKE_CLAUDE_NO_HOOKS=1` and `hook_timeout_secs=1`, the badge becomes `? unknown`.
Evidence: open a 2-repo `fake-claude` project, then `baton debug send 'x/<repo2>' 'perm\r' && sleep 0.5 && baton debug sessions --json | jq -r '.[].status'` lists `Idle` and `Permission`. TUI capture: `drive … --step 'wait:◐' --step 'send:n' --step dump -- target/debug/baton` shows the selection on repo2. Optional with real claude: ask for a Bash command under `--permission-mode default` (costs a few haiku tokens) and see `◐`.

### Task 15: Desktop notifications (M3b)
Language: rust
Goal: The daemon sends a desktop notification when a session **enters** an attention state and at least one of these holds: no TUI is attached, the terminal isn't focused, or that session isn't the one on screen.
Files: `crates/baton-core/src/notify_rule.rs`, `crates/baton/src/daemon/notifier.rs`, `crates/baton/src/tui/event_loop.rs` (FocusGained/FocusLost → `ClientView`), `crates/baton/tests/e2e_notify.rs`
Depends on: Task 14
Acceptance criteria:
- `notify_rule::should_notify(prev, new, view: Option<ClientView>, session, enabled) -> bool` is pure. Table tests cover every combination of: attached or not, focused or not, on screen or not, entering vs remaining in attention, and the disabled flag.
- **Sink:**
  - The `NotificationSink` trait has a `DbusSink` (`notify-rust`, app name `Baton`, summary `<repo>: needs permission` / `<repo>: your turn`, body = the project name).
  - `LogSink` appends `notify <session> <status>` lines to `<state>/notifications.log`. It is selected with `BATON_NOTIFY_SINK=log`, which tests use.
  - D-Bus failures are logged and never propagate.
- **Repeats:** no repeat notification while a session remains in the same attention state. A new one fires after it leaves and re-enters.
- **Focus reporting:** the TUI enables host focus-change reporting and sends `ClientView` on `FocusGained` / `FocusLost`.
- **Config:** `notifications = false` suppresses all notifications.
- **e2e test** (LogSink): with no TUI attached, `perm` produces one log line. With the TUI attached, the session on screen and the terminal focused (baton-drive sends `ESC[I`), `perm` produces no line. With focus lost (`ESC[O`), it produces a line.
Evidence: `BATON_NOTIFY_SINK=log baton daemon start && baton debug open x && baton debug send 'x//tmp' 'perm\r' && sleep 0.5 && cat $D/state/notifications.log` → `notify x//tmp Permission`. Optional real desktop check: run with the default sink while `dbus-monitor --session "interface='org.freedesktop.Notifications'"` shows a `Notify` call (M3 done).

### Task 16: Persistence, launch ladder (resume → continue → fresh) and restart (M4)
Language: rust
Goal: Persist per-session metadata, relaunch conversations with `--resume` after daemon death or restart, fall back gracefully, and implement `r` restart.
Files: `crates/baton-core/src/state.rs` (model + atomic save/load), `crates/baton-core/src/launch.rs` (ladder logic, pure), `crates/baton/src/daemon/spawn.rs`, `crates/baton/src/daemon/registry.rs`, `crates/baton/src/tui/app.rs`, `crates/baton/tests/e2e_resume.rs`
Depends on: Task 14
Acceptance criteria:
- **`state.json`:**
  - Format: `{version, sessions: {<baton id>: {project, repo, profile, claude_session_id, transcript_path, last_status, updated_at}}}`.
  - Written atomically (tmp + fsync + rename) on SessionStart and on every status or exit change, debounced to 250 ms.
  - Loaded at daemon start. A corrupt file is renamed to `state.json.bad-<ts>`, logged, and the daemon starts empty.
  - Persisted sessions without a live process are listed as part of a closed project until opened.
- **Launch ladder** (pure, table-tested):
  - If a `claude_session_id` is known: `--resume <id>`. Otherwise: `--continue`. Last resort: fresh with `--session-id <new uuid v4>` (Finding 8), so the id is known before `SessionStart`.
  - An attempt counts as an **early failure** when the process exits with a non-zero code within 10 s and before any `SessionStart`. On an early failure, the ladder moves to the next rung. A slow trust dialog is not a failure.
  - The rung used is logged and shown in the info panel as `launch resume|continue|fresh`.
- **Restart:**
  - `Restart{session}`, bound to `r`, kills the process group (SIGHUP, then SIGKILL after 3 s), keeps the screen object (clearing it), and relaunches with `--resume <claude_session_id>`, falling back via the ladder.
  - Works on `Exited` sessions too.
  - Pressing `r` asks `Restart <repo>? [y/N]` when the session is `Running` or `Permission`.
- **e2e tests** (`fake-claude` with `FAKE_CLAUDE_LOG`):
  - Open and run `prompt hi`, then `baton daemon stop`. On `debug open x`, the argv log line contains `--resume <same id>`.
  - Delete the transcript and relaunch: the log shows `--resume <id>` (exit 1), then `--continue` (exit 1, no history), then fresh with `--session-id`.
  - `debug restart` (a new `baton debug restart <session>`) relaunches with `--resume`.
Evidence: `baton debug open x && baton debug send 'x//tmp' 'prompt hi\r' && baton debug sessions --json | jq -r '.[0].claude_session_id'`, then `baton daemon stop && baton debug open x && tail -1 $FAKE_CLAUDE_LOG | jq -r '.argv|join(" ")'` shows `--resume <that id>`. Optional with real claude (no tokens): on a repo with prior history, `daemon stop` + reopen shows the previous conversation in `debug screen` (M4 done).

### Task 17: Transcript tailer, usage and cost, and the session info panel (M5)
Language: rust
Goal: Tail each session's transcript, aggregate usage tolerantly (Finding 10), compute context % and estimated cost from the config price table, and render the full info panel.
Files: `crates/baton-core/src/transcript.rs`, `crates/baton-core/src/pricing.rs`, `crates/baton/src/daemon/tailer.rs`, `crates/baton/src/tui/info_panel.rs`, `crates/baton-core/tests/fixtures/transcript_sample.jsonl`, `crates/baton/tests/e2e_usage.rs`
Depends on: Task 16
Acceptance criteria:
- **`UsageAccumulator::feed_line(&str)`:**
  - Ignores non-`assistant` and unparsable lines.
  - **Deduplicates by `message.id`**: a fixture where 3 entries share an id counts that id once.
  - Sums input / output / cache_read / cache_creation across main and sidechain entries.
  - Context % uses only the latest non-sidechain assistant usage: `(input + cache_read + cache_creation) / context_window(model)`.
  - The model id is taken from the latest assistant entry.
- **Context window:** configurable per model prefix under `[pricing]` (`context_window`), defaulting to 200 000.
- **Cost:** `pricing::estimate(model, usage)` uses longest-prefix match over `[pricing.models."<prefix>"]` and falls back to `default`. Rates are $/MTok. The result is unit-tested.
- **Tailer:**
  - Polls every 1 s from the stored byte offset and buffers a partial trailing line.
  - Waits silently while the file doesn't exist.
  - Resets on truncation (size < offset) or when the path changes, e.g. via a new SessionStart.
  - Emits `UsageUpdated` only when the values change.
  - Any error → usage `None` → the panel shows `n/a`. The tailer never affects session status.
- **Info panel** (spec §4): path, profile, model, status (+ exit code when exited), uptime (`1h12m`), context bar `███████░░░ 68%`, tokens `1.2M in / 84k out` + cache line, cost `~$4.10 (est.)`, id abbreviated `7f3c…a91e`, launch rung. Rendering is unit-tested with `TestBackend` for both full and `n/a` states.
- **e2e test:** `fake-claude` `prompt hi` twice gives tokens `in=200 out=100 cache_read=2000 cache_write=20`. This counts once per `message.id` even though each prompt writes two entries. Context % = (100+1000+10)/200000 and cost both match their formulas.
Evidence: after `baton debug send 'x//tmp' 'prompt hi\r'` twice and `sleep 1.5`, `baton debug sessions --json | jq '.[0].usage'` shows `input: 200, output: 100`. The TUI capture `drive … --step 'wait:est\.' --step dump -- target/debug/baton` shows the info panel with `context`, `tokens`, `cost ~$… (est.)`. Optional with real claude: one short prompt (a few cents with haiku) updates the panel live (M5 done).

### Task 18: Remappable keybindings, help overlay and `e` editor (M6)
Language: rust
Goal: Make every action in spec §4 remappable via `[keybindings.normal]` / `[keybindings.focus]`, add the `?` help overlay listing the active bindings, and open the repo in the configured editor.
Files: `crates/baton-core/src/keymap.rs` (key string parser + action enums + defaults + conflict check), `crates/baton/src/tui/keys.rs` (KeyEvent → KeySpec matching), `crates/baton/src/tui/help.rs`, `crates/baton/src/tui/editor.rs`, `crates/baton/src/tui/app.rs`, `crates/baton/tests/e2e_keys.rs`
Depends on: Task 17
Acceptance criteria:
- **Key-string parser:** accepts `"n"`, `"G"`, `"?"`, `"enter"`, `"esc"`, `"tab"`, `"up"`, `"pgup"`, `"f5"`, `"ctrl-u"`, `"alt-n"`, `"alt-1"`, `"ctrl-\\"` and `"shift-tab"`, case-insensitive for named keys. Invalid strings produce an error naming the config key.
- **Matching:** `ctrl-\` matches both crossterm forms (Task 6), and `G` matches `Char('G')` with or without the SHIFT modifier. Table tests cover the parser and the matching.
- **Actions:**
  - Normal mode: `move_down`, `move_up`, `activate`, `open_project`, `select_1..9`, `next_attention`, `restart`, `editor`, `scroll_up`, `scroll_down`, `page_up`, `page_down`, `live`, `help`, `quit`.
  - Focus mode: `unfocus`, `next_attention`, `session_1..9`.
  - Defaults match spec §4. Config entries override per action and accept a string or a list of strings.
- **Conflicts:** two actions bound to the same key in one mode is a config error. Binding `focus.unfocus` to a plain printable key is rejected with an explanation (it would swallow typing).
- **Help overlay:** `?` lists the active bindings for both modes from the live keymap (no hardcoded text). `?` or `Esc` closes it.
- **Editor:** `e` runs the `editor` template, replacing `{path}` with the repo path and splitting with `shell-words`. It spawns detached (setsid, null stdio, not waited on, zombie reaped by a background `wait`). Spawn errors show in the bottom bar for 5 s.
- **e2e test:** config with `[keybindings.normal] next_attention = "x"` and `[keybindings.focus] unfocus = "ctrl-g"`. `x` jumps to an attention session, `n` no longer does, and `\x07` (Ctrl-g) leaves focus mode. `editor = "sh -c 'echo {path} > $D/edited'"` + `e` writes the repo path to the file. `?` shows `next_attention  x`.
Evidence: `drive --size 40x120 --step 'wait:NORMAL' --step 'send:?' --step 'wait:next_attention +x' --step dump -- target/debug/baton` shows the remapped binding. `test "$(cat $D/edited)" = /tmp` after pressing `e` on session `x//tmp` (M6 done).

### Task 19: `baton doctor` (hook contract check)
Language: rust
Goal: Implement the spec's Risk 3 mitigation: a self-check that the environment and each profile's `claude` actually fire Baton's injected hooks, so `unknown` statuses can be diagnosed.
Files: `crates/baton/src/cmd/doctor.rs`, `crates/baton/tests/e2e_doctor.rs`
Depends on: Task 18
Acceptance criteria:
- **Checks**, each printed as a `PASS` / `WARN` / `FAIL` line:
  1. Config parses.
  2. Runtime/state dirs are writable and the runtime dir mode is 0700.
  3. Daemon is reachable and the protocol version matches (`WARN` if not running).
  4. For each profile: the command resolves on `PATH`, and `<cmd> --version` output is captured. Real `claude` prints `2.1.295 (Claude Code)`.
  5. For each profile: a hook probe. It spawns the profile command in a PTY, in the first configured repo using that profile (so folder trust matches real use), with `--settings <tmp probe hooks.json>`. The probe's `SessionStart` hook command is `sh -c 'cat > <tmp>/probe.json'`. Within 20 s, `probe.json` must contain `hook_event_name == "SessionStart"` and a `session_id`. Then it sends SIGTERM and SIGKILL to the probe. On timeout the line is `FAIL` with the hint `trust dialog pending or hooks disabled (disableAllHooks / --bare)?`.
  6. `notify-send` / D-Bus session bus availability (`WARN` only).
- **Exit status and options:** exit 0 if there are no `FAIL`s, 1 otherwise. `--no-probe` skips check 5.
- The probe never sends a prompt, so it costs no tokens. This was verified in Finding 1.
- **e2e test:** a `fake-claude` profile → all `PASS`, exit 0. With `FAKE_CLAUDE_NO_HOOKS=1` in the profile env → hook probe `FAIL`, exit 1.
Evidence: `baton doctor` with a `fake-claude` profile prints `PASS hooks: SessionStart received (profile p)` and exits 0. Optional with real claude (no tokens): `BATON_CONFIG=~/.config/baton/config.toml baton doctor` shows `PASS hooks …` for both work and personal profiles on trusted repos.
