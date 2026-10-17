# Project Summary in the Main Pane Plan
Status: APPROVED

## Goal
Follow-up to `docs/plans/project-summary-pane.md`, which is already implemented on this branch. When the sidebar cursor is on a project row in normal mode, the big main pane shows the project summary instead of mirroring the last-shown session's terminal. The main block is titled `Project <name>` and shows the same tally line and per-session entries (number, badge, repo, status, branch, profile). The lower sidebar block always goes back to the `Session` info panel, so the same content is not shown twice. Since the main pane no longer shows a session in this state, the TUI also stops telling the daemon that a session is "on screen". Keys that act on the shown session (`r`, `e`, `z`, scroll keys) do nothing on a project row and show a hint instead of acting on a session the user cannot see. No protocol, daemon or config changes are needed.

## Findings (read from the repo on 2026-10-18)
1. **Drawing.**
   - `ui.rs::draw` draws the main pane's inner area (`l.inner`) as one of: the scrollback `View`, the `selected_mirror()` screen, or the `No sessions…` paragraph.
   - `draw_chrome` draws the main block's border and title (`repo · ⎇ branch · profile · badge status [scrollback -N]` from `selected_session()`, which is `App.current`). Since the previous plan, it also switches the lower sidebar block between `Project <name>` (`project_summary::lines`) and `Session` (`info_text`).
   - Fullscreen skips `draw_chrome` and draws the mirror into the borderless `l.inner`.
2. **Reusable model.** `project_summary::lines(entries, width, height)` is pure and works for any size. On the default 120x40 host, the main inner area is about 86x37, which holds 18 two-line entries before `+N more`. The extra room adds no new data (model, context %, cost are per-session details already in the `Session` panel). So the plan reuses `lines` unchanged and keeps it small.
3. **Key routing** (`app.rs::on_key`, `apply_event`):
   - In **normal** mode, keys go only to `keymap.normal_action`. Nothing is forwarded to a session.
   - Mouse, paste and focus events are forwarded only in **focus** mode.
   - Focus mode is entered only by `activate` on a session row. `Enter` on a project row steps into its first session or opens the project.
   - In focus mode, the cursor moves only through `pick` / `next_attention`, which always `go_to` a session.
   - So typed text never reaches a session while the cursor is on a project row in normal mode. The only edge case is `settle()` after a session list change, which can move a focused cursor elsewhere. That is why the summary is gated on `mode == Normal`: in focus mode the pane always mirrors the session that receives the keys.
   - However, these **normal actions act on `current`, the hidden session**:
     - `r` (`ask_restart`: restarts at once when idle);
     - `e` (`open_editor`);
     - `Ctrl-u/d/b/f` / live (`scroll_by`);
     - `z` (`toggle_fullscreen`).

     Today the main title names that session. Once the main pane shows the summary, nothing names it.
4. **On-screen reporting matters.**
   - `sync_view` sends `ClientView { on_screen: selected_session() }`.
   - The daemon (`daemon/session.rs::apply`) treats an on-screen session as viewed: a `Stop` (YourTurn) is cleared at once.
   - `notify_rule::should_notify` skips the desktop notification for the on-screen session.
   - If the summary replaced the mirror but `on_screen` still named `current`, that session could finish its turn unseen, with no attention badge and no notification. So `on_screen` must be `None` while the summary is displayed.
5. **Existing tests that change.**
   - `ui.rs`: `project_row_shows_the_project_summary`, `collapsed_project_summary_lists_hidden_sessions`, `project_summary_shows_the_branch` (they assert the summary in the sidebar, using `│` prefixes).
   - `app/tests.rs`: `summary_project_is_set_only_on_a_project_row` (extended).
   - `e2e_keys.rs`: `project_row_shows_a_summary_of_its_sessions` (asserts that the `Project x` block is the lower sidebar block, and `Session` after `j`).
   - Tests that keep holding, because they only press keys on session rows:
     - `cursor_starts_on_the_first_session_and_walks_projects_and_sessions` (`current` is still kept on project rows);
     - the fullscreen e2e;
     - the remapped-keys e2e (`e` on a session row).

## Decisions
- **Main pane shows the summary** when `App::summary_project()` is `Some`. That accessor now returns the cursor's project only when all of these hold: the cursor is `Cursor::Project`, `mode == Normal`, and not fullscreen. Then:
  - the main block title is `Project <clean(name)>` (no scrollback marker), with the normal-mode border style;
  - `l.inner` gets `project_summary::lines(entries, inner.width, inner.height)`, with entries built exactly as in the sidebar today. A closed project with no sessions shows `no sessions`.
  - `App.current`, its mirror and its scroll state are kept untouched. Moving back onto a session row shows that session's mirror again, as before.
- **Lower sidebar block reverts to `Session` always**, showing info for `current` as before the previous plan. Reasons:
  - showing the same summary twice wastes the 16-row block;
  - the 30-column version was cramped;
  - the `Session` block keeps a stable meaning ("the last session you looked at").

  *(User may veto: "keep both" is the alternative. It is less work: skip the sidebar part of Task 2.)*
- **`on_screen` follows what is drawn.** New `App::on_screen_session()` returns `None` while `summary_project()` is `Some`, and `selected_session()` otherwise. `sync_view` uses it, so moving onto and off a project row each sends one `ClientView`.
- **Session-targeting keys on a project row** (only when `summary_project()` is `Some`):
  - `r`, `e`, the five scroll actions and `z` do nothing and set the hint `select a session first` (new `SELECT_SESSION_HINT`).
  - `z` / `Esc` while already fullscreen still leave fullscreen. `summary_project()` is `None` in fullscreen, so the gate never applies there. Moving with `j`/`k` in fullscreen keeps mirroring `current`, as today.
  - `o`, `Enter`/`l`, `Space`, digits, `n`, `?` and `q` are unchanged.

  *(User may veto: leaving them acting on the hidden `current` is the alternative.)*
- **No richer content.** Per-session model, context and cost already live in the `Session` panel. A wider one-line-per-entry layout can come later if the user wants it.
- **No new keys, config or protocol changes.** `project_summary.rs` is unchanged.

### Task 1: App state for a summary-showing main pane
Language: rust
Files: crates/baton/src/tui/app.rs, crates/baton/src/tui/app/tests.rs
Depends on: none
Acceptance criteria:
- `summary_project()` returns `Some("x")` on project row `x` in normal mode. It returns `None` in each of these cases, each checked in `summary_project_is_set_only_on_a_project_row` (extended):
  - on a session row;
  - with no cursor;
  - in focus mode;
  - when fullscreen (entered on a session row, then `k` onto the project row).
- New test `on_screen_is_none_while_a_project_summary_is_shown`, on `tree_app()`:
  - pressing `k` onto project `x` emits exactly one `ClientView { on_screen: None, .. }` (checked with the existing `views()` helper);
  - pressing `j` back emits `ClientView { on_screen: Some(x//a), .. }`;
  - `shown(&app)` stays `Some("x//a")` throughout.
- New test `session_keys_on_a_project_row_only_hint`: on project row `x`, each of `r`, `e`, `Ctrl-u`, `Ctrl-d` and `z` returns no effects, leaves `is_fullscreen()` false and sets `app.hint == Some(SELECT_SESSION_HINT)`. On session row `x//a` (idle), `r` still returns `Restart { session: x//a }`.
- On project row `x`, `o`, `Enter` and `Space` behave as their existing tests assert. Those tests pass unchanged.
- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test -p baton-tui` pass.
Evidence: none — App state and effects only; the visible change lands in Task 2.

### Task 2: Draw the summary in the main pane; sidebar back to Session
Language: rust
Files: crates/baton/src/tui/ui.rs, docs/SPEC.md
Depends on: Task 1
Acceptance criteria:
- `draw`: when `app.summary_project()` is `Some`, `l.inner` gets a `Paragraph` of `project_summary::lines(..)` sized to `l.inner`. Otherwise, the existing scroll view / mirror / `No sessions` logic runs.
- `draw_chrome`:
  - the main block title is `Project <name>` while the summary is shown, and the session title otherwise (unchanged);
  - the lower sidebar block is always `Session` with `info_text` (the sidebar summary branch is removed).
- ui tests, rewritten or added:
  - `project_row_shows_the_project_summary_in_the_main_pane` (with profile `p`, after `k`):
    - the render contains `┌Project x` as the main block title, in the main pane region (column > 32);
    - it contains `1 work` and `1 ● a  running` at column > 32;
    - it contains the `Session` title and `profile  p` in the sidebar;
    - it does not contain `mirrored text` or `a · p · ● running`.
  - `collapsed_project_summary_lists_hidden_sessions`: the main pane lists `1 ● a` and `2 ● b` while the sidebar shows `▸ x  (2)`.
  - `project_summary_shows_the_branch`: the main pane contains `⎇ main · default` (asserted on the column > 32 part of the lines).
  - `session_row_keeps_the_session_info` and `main_title_and_info_panel_describe_the_session` pass, and also assert `mirrored text`.
  - New `focus_mode_never_shows_the_summary`: focus mode set and cursor forced to a project row through a session-list change that removes the focused session (or directly in a `#[cfg(test)]` setup). The render still contains the mirror, not `Project x`.
  - The fullscreen tests (`fullscreen_has_no_pane_highlight`, the borderless tests) pass unchanged.
- `docs/SPEC.md`:
  - §3 **Project summary**: the heading says it is shown in the main pane when the cursor is on a project row in normal mode.
  - §4 Layout: the sentence becomes "When the cursor is on a project row (normal mode), the main pane is titled `Project <name>` and shows the project summary instead of a session; the `Session` block keeps describing the last shown session. `r`, `e`, `z` and the scroll keys need a session row there."
  - The `r`/`e`/`z`/scroll rows of the key table mention "shown session (not on a project row)".
  - Where SPEC describes `ClientView` / on-screen notifications, add that nothing is on screen while a project summary is shown.
- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test -p baton-tui` pass.
Evidence: `cargo test -p baton-tui tui::ui` passes the tests above. In a real terminal, in `baton` with a project open, pressing `k` up to `▾ <project>` makes the main pane read `Project <project>`, with a tally and per-session entries. The sidebar still shows `Session`. Pressing `r` shows `select a session first` in the bar. Pressing `j` brings back the session's live terminal.

### Task 3: End-to-end check
Language: rust
Files: crates/baton/tests/e2e_keys.rs
Depends on: Task 2
Acceptance criteria:
- `project_row_shows_a_summary_of_its_sessions` is rewritten for the main pane, using `Env::new("")`:
  1. Before opening, with the cursor on closed project `x`, the screen shows `Project x` and `no sessions`.
  2. After `o`, it waits for `1 ○ alpha  idle` and `2 ○ beta  idle`.
  3. It presses `k` until on `▾ x` (at most 3 presses). The screen then shows `Project x` and `2 idle`. Some line contains `1 ○ alpha  idle` right of column 32 (the main pane), and `  p` follows there with no `⎇`. No line starts with `FAKE CLAUDE`. The sidebar still contains `Session`.
  4. It sends `r`. The bottom row contains `select a session first`.
  5. It presses `j`. It waits for a `FAKE CLAUDE` line and `alpha · p · `. The screen no longer contains `Project x`.
- The other e2e tests pass unchanged. The full gate is green: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test`.
Evidence: `cargo test -p baton-tui --test e2e_keys project_row_shows_a_summary` passes. `baton-drive --size 40x120 --step 'wait:idle' --step 'send:k' --step 'sleep:300' --step dump -- baton` (with a fake-claude config) dumps a main pane titled `Project x` that lists each session with its status and profile, next to the `Session` sidebar block.
