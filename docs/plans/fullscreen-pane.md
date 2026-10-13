# Fullscreen Session Pane Plan
Status: APPROVED

## Goal
Add a "fullscreen" view toggle to the baton TUI. It draws the shown session's mirror over the whole host terminal except the last row, with no sidebar, info panel, border or title. The bottom bar stays, so the mode indicator and the key that enters and leaves fullscreen stay visible. The session PTY is resized to the full host width and the host height minus the bar row, and terminal mouse capture is turned off while the view is active. Together these let the user select and copy multi-line Claude output with the terminal's own mouse selection, with no sidebar text and no box-drawing characters in the copied text.

The toggle keys are:
- `z` and `Alt-z` in normal mode.
- `Alt-z` in focus mode. Claude Code does not use it, and neither does any existing baton binding.

Both are remappable as the `fullscreen` action. `Esc` also leaves fullscreen in normal mode. The help overlay, bottom bars (which are still shown in fullscreen), `docs/SPEC.md`, `README.md` and `examples/config.toml` document the keys.

## Findings (read from the repo on 2026-10-13)
1. **Keymap.** Key bindings live in `crates/baton-core/src/keymap.rs`. There are two per-mode action enums, `NormalAction` and `FocusAction`, each implementing the `Action` trait (`all`, `name`, `default_keys`).
   - `build_table` rejects two actions on one key within a mode.
   - The help overlay (`crates/baton/src/tui/help.rs`) lists `describe_normal()` and `describe_focus()` automatically, with the action name padded to `NAME_WIDTH = 16`. That is why the action is named `fullscreen` (10 chars) and not `toggle_fullscreen` (17 chars, which would run into the key column).
2. **Keys already taken.**
   - Normal-mode defaults: `j k ↓ ↑ enter l o space 1-9 n r e ctrl-u ctrl-d pgup pgdn G ? q`.
   - Focus-mode defaults: `ctrl-\`, `alt-n`, `alt-1..9`. Every other key is forwarded to `claude`.
   - Claude Code's documented shortcuts (code.claude.com/docs/en/interactive-mode, fetched today) use `Alt+B D F M O P T V Y`, `Alt+Up/Down`, many `Ctrl+` letters (including `Ctrl+Z` and `Ctrl+[`), `Shift+Tab`, `Shift+Enter` and `Esc`.
   - **`Alt+Z` is unused**, and it echoes tmux's `prefix z` zoom.
   - F11 is rejected because terminal emulators (GNOME Terminal, Konsole) usually grab it for their own fullscreen.
   - `Esc` cannot exit fullscreen in focus mode, because Claude needs it (interrupt or cancel). It is only a fullscreen exit in normal mode, where it is unbound today.
3. **Layout.** `crate::spike::layout(area)` builds the `Layout { sidebar, main, inner, bar }` that `App` stores. The bar is the last host row (`bar_h = 1.min(area.height)`), so a 40x120 host gives a 37x86 `inner` today.
   - `App::fit(host)` recomputes it and sends `ClientMsg::Resize` when `inner` changes size; it also resizes the mirrors and clears cached scrollback.
   - `App` does not remember `host`, so a key-driven toggle needs it stored.
4. **Rendering.** `crates/baton/src/tui/ui.rs::draw` always draws the sidebar, the info panel, the bordered main block with its title, the mirror or scroll view into `l.inner`, the bottom bar, and then the overlays.
   - The confirm prompt (`r` on a running session), flash messages and hints live only in the bottom bar. While one is shown it replaces the key list, but the ` NORMAL │ ` / ` FOCUS │ ` prefix stays.
   - The bar text itself contains `│` (after the mode name), so "no box-drawing characters" checks in fullscreen must look only at the rows above the bar.
   - The test `default_bars_match_the_spec_text` pins the bar text to `docs/SPEC.md`.
5. **Mouse.** `crates/baton/src/tui/terminal_guard.rs` enables `EnableMouseCapture` once in `TerminalGuard::enter`, and `restore()` disables it, including on panic.
   - In focus mode, `App::on_mouse` forwards mouse events inside `layout.inner` to the session, encoded per the child's mouse mode.
   - With capture on, most terminals only do native selection with Shift-drag, and that selection still crosses the sidebar and borders. This is why capture must be off in fullscreen.
   - With capture off on the alternate screen, many terminals (VTE, kitty, alacritty, foot, xterm with `alternateScroll`) turn the mouse wheel into Up/Down arrow keys (DECSET 1007, "alternate scroll"). Those keys would move the sidebar cursor in normal mode and Claude's prompt history in focus mode. So 1007 is reset while in fullscreen.
6. **Event loop.** `crates/baton/src/tui/event_loop.rs::drive` owns the `TerminalGuard` and performs `Effect`s. It is the natural place to bring the terminal's mouse state in line with what `App` wants after each step.
7. **End-to-end test harness.** `baton-testkit`'s `Drive` keeps a `vt100::Parser`. `vt100::Screen::mouse_protocol_mode()` exposes whether the TUI asked for mouse reporting, so e2e tests can assert that capture is off.
   - At start, `fake-claude` prints `DA1: …` and `FAKE CLAUDE session=…`. In fullscreen those lines start at column 0 instead of after the 32-column sidebar and the border. The last row still holds the bar.

## Decisions
- **Fullscreen is a view flag, separate from the input mode.**
  - It keeps the current mode (`Normal` or `Focus`). Normal-mode keys such as `j`/`k`/`n`/`1-9` keep working and switch the session shown fullscreen.
  - The mode indicator stays visible, because the bottom bar is kept.
  - `Ctrl-\` (unfocus) does not leave fullscreen.
- **Keys.**
  - `NormalAction::Fullscreen` defaults to `["z", "alt-z"]`; `FocusAction::Fullscreen` defaults to `["alt-z"]`. Both are named `fullscreen` in `[keybindings.normal]` and `[keybindings.focus]`.
  - In normal mode, `Esc` also exits fullscreen, unless the user bound `esc` to a normal action.
  - The toggle works only when a session is shown. With no session it does nothing and sets a hint.
- **Exact pane size.** The PTY is resized to the full host width and the host height minus the bar row (`(rows - 1) × cols`, saturating at 0 on a 0-row host). Claude then wraps at the real width, so fewer lines are soft-wrapped and copied lines are full width.
  - Leaving fullscreen resizes back.
  - Cached scrollback is cleared on each toggle, as `fit` already does.
  - Other attached clients see the resize, the same as with a host resize today.
- **The bottom bar is always drawn, fullscreen or not.**
  - It is the same bar as outside fullscreen: mode indicator, key list (now including the fullscreen key), confirm prompt, flash messages and hints.
  - So a restart confirmation (`r`) and editor error flashes no longer leave fullscreen; they appear in the bar and the view stays fullscreen. Answering the confirmation also keeps fullscreen.
  - Plain hints (for example "no session needs attention") are shown in the bar as usual.
  - Overlays (help, disconnected, version mismatch, config error) are still drawn on top.
  - Connection loss, a protocol mismatch and a reconnect still reset fullscreen to off, because the shown session may no longer exist.
  - **Copy caveat:** a drag that runs into the last row also selects bar text. The user stops the drag above the bar; the README says so.
- **Mouse.**
  - While fullscreen is on, mouse capture is disabled and DECSET 1007 is reset (`CSI ? 1007 l`).
  - On leaving fullscreen, capture is re-enabled and `CSI ? 1007 h` is sent. `restore()` also sends `CSI ? 1007 h` if baton had reset it, so the terminal's default comes back after exit.
  - **Caveat:** on xterm, where 1007 is off by default, re-enabling it turns it on. This is an accepted trade-off against querying the terminal with DECRQM. *(User may veto: the alternative is to leave 1007 alone and accept wheel-to-arrow translation in fullscreen.)*
  - **Cost:** while fullscreen, Claude receives no mouse events (no wheel scroll or clicks in Claude's own fullscreen renderer). The user toggles back for that.
- **How the mouse state is applied.** It is a reconciliation, not an `Effect`. `App::wants_mouse_capture()` returns `!fullscreen`. The event loop tracks the last applied state and calls a new `terminal_guard::set_mouse_capture(out, on)` when the two differ. This covers every path that changes `fullscreen`, including `on_disconnected`, which returns no effects.
- **Known limitation, documented.** Copying across lines that Claude soft-wrapped yields one line per screen row, because baton redraws cells and does not pass through the original wrapping. The full-width pane reduces how often this happens.

### Task 1: Keymap `fullscreen` action
Language: rust
Files: crates/baton-core/src/keymap.rs
Depends on: none
Acceptance criteria:
- `NormalAction::Fullscreen` exists with name `fullscreen` and default keys `z`, `alt-z`. `FocusAction::Fullscreen` exists with name `fullscreen` and default key `alt-z`. Both are included in `all()`.
- `Keymap::default()` builds without conflict; `normal_action("z")`, `normal_action("alt-z")` and `focus_action("alt-z")` return the fullscreen actions.
- Overrides `[keybindings.normal] fullscreen = "f"` and `[keybindings.focus] fullscreen = "f12"` take effect. A focus override that collides with `next_attention` returns `KeymapError::Conflict`.
- `describe_normal()` and `describe_focus()` contain `("fullscreen", "z, alt-z")` and `("fullscreen", "alt-z")`.
- The existing test that checks every default key parses still passes, and `cargo test -p baton-core` is green.
Evidence: `baton config check` on a config with `[keybindings.focus] fullscreen = "f12"` exits 0, and with `fullscreen = "alt-n"` exits 1 with a `keybindings.focus` conflict message.

### Task 2: Fullscreen state, layout and resize in the reducer
Language: rust
Files: crates/baton/src/spike.rs, crates/baton/src/tui/app.rs, crates/baton/src/tui/app/tests.rs
Depends on: Task 1
Acceptance criteria:
- `spike::fullscreen_layout(area)` returns a zero-size `sidebar`, the same `bar` as `layout(area)` (last row, full width), and `main == inner ==` the area above the bar. A unit test checks that a 40x120 host gives `inner` 39x120 at (0,0) and `bar` 1x120 at row 39, and that a 0x0 and a 1x1 host do not panic.
- `App` stores the last host `Rect` (set in `new` and `fit`) and a `fullscreen: bool`, exposed through `is_fullscreen()`. `fit` uses `fullscreen_layout` when the flag is set.
- With a session shown, `z` in normal mode, or `Alt-z` in focus mode, flips the flag and returns `Send(Resize { rows: 39, cols: 120 })` for a 40x120 host. Pressing it again returns `Resize { rows: 37, cols: 86 }`. The mode does not change.
- In focus mode, `Alt-z` is not forwarded to the session (no `Input` effect), and `Esc` is still forwarded as `Input([0x1b])` while fullscreen.
- In normal mode, `Esc` while fullscreen leaves fullscreen. `Esc` while not fullscreen does nothing.
- With no session shown, the toggle leaves `is_fullscreen()` false and sets a hint.
- Starting a restart confirmation (`r` on a running session) while fullscreen keeps `is_fullscreen()` true, returns no `Resize`, and `confirm_prompt()` is `Some`. Answering it (yes or no) also keeps fullscreen.
- `on_editor_error` while fullscreen keeps `is_fullscreen()` true, returns no `Resize`, and `flash()` is `Some`.
- `on_disconnected`, `on_mismatch` and `on_connected` clear fullscreen.
- In fullscreen focus mode, mouse events at (0,0) are forwarded with origin (0,0).
- `App::wants_mouse_capture()` is `!is_fullscreen()`.
Evidence: none — reducer state only; the visible behavior is exercised in Task 3 and Task 4.

### Task 3: Borderless fullscreen rendering, bars and help
Language: rust
Files: crates/baton/src/tui/ui.rs, crates/baton/src/tui/help.rs, docs/SPEC.md
Depends on: Task 2
Acceptance criteria:
- When `is_fullscreen()`, `ui::draw` renders the mirror (or the scrollback view) into `l.inner` (every row but the last, full width) and the usual bottom bar into `l.bar`; the sidebar, info panel, main border and title are not drawn. In a `TestBackend` 120x40 render of a fullscreen app:
  - rows 0..39 contain no `Projects`, no `Session` panel title, and none of `┌ ┐ └ ┘ │ ─`, and row 0 starts with the mirror's first row text;
  - row 39 is the bar: it starts with ` NORMAL │ ` in normal mode (and ` FOCUS │ ` in focus mode) and contains `z fullscreen` (normal) or `Alt-z fullscreen` (focus).
- In fullscreen, a pending restart confirmation renders in row 39 as ` NORMAL │ <prompt>`, and an editor error flash renders there too, with the view still borderless.
- Overlays still render in fullscreen: the help overlay test passes with fullscreen on.
- Not-fullscreen rendering is unchanged, and the existing ui tests pass.
- `normal_bar` gains `z fullscreen` (before `? help`) and `focus_bar` gains `Alt-z fullscreen` (at the end). `default_bars_match_the_spec_text` is updated, and `docs/SPEC.md` (the bar lines and both default-keybinding tables, with a `z` / `Alt-z` row) matches it character for character.
- The help overlay lists `fullscreen` in both columns, and a remapped key appears there.
- The help title mentions nothing new. Its existing `? or Esc to close` still holds: `Esc` closes help before it would exit fullscreen.
Evidence: `baton-drive --size 40x120 --step 'wait:idle' --step 'send:z' --step 'sleep:300' --step dump -- baton` (with a fake-claude config) shows a dump with no sidebar or borders, `FAKE CLAUDE session=` at column 0, and a last row starting with ` NORMAL │ ` that contains `z fullscreen`.

### Task 4: Disable mouse capture and alternate scroll in fullscreen
Language: rust
Files: crates/baton/src/tui/terminal_guard.rs, crates/baton/src/tui/event_loop.rs, crates/baton-testkit/src/drive.rs, crates/baton/tests/e2e_keys.rs
Depends on: Task 3
Acceptance criteria:
- `terminal_guard::set_mouse_capture(out: &mut impl Write, on: bool)` behaves as follows:
  - `on = false` writes crossterm's `DisableMouseCapture` sequence followed by `\x1b[?1007l`, and records that 1007 was reset.
  - `on = true` writes `EnableMouseCapture` and `\x1b[?1007h`.
  - `restore()` writes `\x1b[?1007h` only if 1007 is still recorded as reset.

  Unit tests check the bytes written to a `Vec<u8>`.
- `drive` keeps a `captured` flag (initially `true`). After each `apply`, it calls `set_mouse_capture` only when `app.wants_mouse_capture() != captured`. A unit-testable helper decides when to switch.
- `Drive::mouse_reporting() -> bool` returns whether `vt100` reports a mouse protocol mode other than `None`.
- The new e2e test in `crates/baton/tests/e2e_keys.rs` runs this sequence:
  1. Open the project; `mouse_reporting()` is true.
  2. Press `z`. The screen loses `Projects` and the borders, a line starts with `FAKE CLAUDE`, the last row still starts with ` NORMAL │ ` and contains `z fullscreen`, and `mouse_reporting()` is false.
  3. Press `Esc`. The sidebar is back and `mouse_reporting()` is true.
  4. Press `Enter` (focus), then `\x1bz` (Alt-z). The view is fullscreen with mouse reporting off, and the last row starts with ` FOCUS │ ` and contains `Alt-z fullscreen`.
  5. Type `hi\r`. It reaches the session (as the existing tests check).
  6. Press `\x1bz` again. The layout is restored.
- The test passes headless under `cargo test --locked`, and the whole gate (`cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test`) is green.
Evidence: `cargo test -p baton --test e2e_keys fullscreen` passes. Manually in a real terminal: run `baton`, focus a session and press `Alt-z`. A plain mouse drag then selects only Claude's text, and pasting shows no `│` or sidebar text as long as the drag stays above the bottom bar.

### Task 5: User documentation
Language: rust
Files: README.md, examples/config.toml, crates/baton-core/tests/config_examples.rs (only if it enumerates actions)
Depends on: Task 4
Acceptance criteria:
- `README.md` gains a "Copying text from a session" section. It covers:
  - `z`/`Alt-z` toggle fullscreen, and `Esc` (normal mode) also exits;
  - the pane has no border or sidebar and mouse capture is off, so a plain drag selects; the bottom bar stays (showing the mode and the fullscreen key), so stop the drag above it;
  - Claude gets no mouse events while fullscreen;
  - soft-wrapped lines copy as separate lines;
  - the keys are remappable as `fullscreen`.
- `examples/config.toml` shows `fullscreen = "z"` under `[keybindings.normal]` and `fullscreen = "alt-z"` under `[keybindings.focus]`, and still parses (the `config_examples` test stays green).
- `cargo test` is green.
Evidence: `baton config check` with `BATON_CONFIG=examples/config.toml` exits 0; `grep -n fullscreen README.md examples/config.toml` shows the new entries.
