# Project Summary Pane Plan
Status: APPROVED

## Goal
When the sidebar cursor is on a project row (for example `▾ twicecat`), the lower sidebar block shows a summary of that project's sessions instead of the info for the shown session. Its title becomes `Project twicecat`. The block starts with a tally line made from the user's idle / work / wait wording (`1 work · 1 wait · 1 idle`). Below that, each session of the project gets one entry with its number, status badge, repo name, status label, git branch and profile (the Claude "type": `work`, `personal`, or any other configured profile, `default` when none). With this, the user can see the state of a whole project from one row, including a collapsed project whose sessions are hidden in the list. When the cursor is on a session row, the block is the `Session` info panel, as it is today. The summary uses only data the TUI already has: `SessionInfo` (project, repo, profile, status), `App::branch`, and the existing `badge`/`status_label`/`needs_attention` helpers. It needs no protocol or daemon changes.

## Findings (read from the repo on 2026-10-16)
1. **Where the pane lives.** baton has only a TUI (no GUI crate). The sidebar is drawn by `crates/baton/src/tui/ui.rs::draw_chrome`, which splits `l.sidebar` (32 columns, `spike::SIDEBAR_WIDTH`) vertically into two blocks:
   - `list_area`: the `Projects` block, with rows from `App::rows()` built by `sidebar::rows`;
   - `info_area`: the `Session` block, `INFO_HEIGHT = 16` rows, 30 inner columns at 32 wide.

   `info_text(app, width)` fills the `Session` block from `app.selected_session()` and shows `no session` when there is none.
2. **Cursor and shown session are different things.**
   - `App.cursor: Option<Cursor>` is `Cursor::Project(name)` or `Cursor::Session(id)`.
   - `App.current` is the session shown in the main panel. When the cursor moves onto a project row, `current` keeps the last session (`go_to` only changes it for session rows).
   - So today, a project row under the cursor still shows the previous session's info, and nothing describes the project itself.
   - `App::cursor_project()` is private and also resolves session rows to their project. `App::cursor()` exists only under `#[cfg(test)]`.
3. **Data at hand.**
   - `SessionInfo { project, repo, profile: Option<String>, status: Status, .. }` (`crates/baton-proto/src/msg.rs`).
   - `App::branch(&SessionInfo) -> Option<&str>` covers every session in `App.sessions`, including remembered `Closed` ones, because `refresh_branches` iterates all of them.
   - `sidebar::repo_name`, `labels::badge`, `labels::status_label`, `baton_core::attention::needs_attention` (Permission | YourTurn).
   - `info_panel::clean` (strips control and bidi characters) and `info_panel::clip` are private helpers that the summary needs too, because branch names and paths come from outside.
4. **The main panel title** already shows `repo · ⎇ branch · profile · badge status` for the shown session, with `default` for a missing profile. The summary uses the same vocabulary.
5. **Status vocabulary.** The `Status` variants are Starting, Running, Permission, YourTurn, Idle, Exited, Unknown and Closed. The user's words map onto them as follows:
   - work = Starting, Running, Unknown (the process is running, but no hook events have been seen yet);
   - wait = `needs_attention` (Permission, YourTurn);
   - idle = Idle;
   - off = Exited, Closed.
6. **Conventions** (`projects-focus-color.md`, `fullscreen-pane.md`):
   - Yellow + BOLD marks attention on row text. DarkGray marks secondary text. DIM marks closed things.
   - `ui.rs` tests render `TestBackend::new(120, 40)` through `render()` with `app_with_session()`.
   - e2e tests in `crates/baton/tests/e2e_keys.rs` use `Env` (project `x` with repos `alpha` and `beta`, profile `p`, fake-claude) and `Drive`.
   - `docs/SPEC.md` §3 and §4 describe the info panel and the layout.
   - Fullscreen skips `draw_chrome` entirely, so this feature has no effect there.

## Decisions
- **Same block, switched content.** The lower sidebar block shows the project summary when the cursor is a `Cursor::Project`, and the session info otherwise. The layout, size and `INFO_HEIGHT` do not change. The main panel keeps showing `current` (unchanged behavior). Cost: while the cursor is on a project row, the shown session's info panel is not visible. Moving to any session row brings it back. *(User may veto: the alternative is a third sidebar block, which shrinks the project list.)*
- **Title** `Project <name>` instead of `Session`. Its style is the default, the same as the `Session` title (only the Projects block gets the active-pane highlight).
- **Content**, at most `width` (30) cells per line. Entries:
  - Line 1, the tally: non-zero groups only, in the order `work · wait · idle · off`, for example `2 work · 1 wait`. When the project has no sessions, line 1 is `no sessions` and there are no other lines.
  - Then one two-line entry per session, in the same order and with the same numbering as the sidebar:
    - `{n} {badge} {repo}  {status_label}`, styled Yellow + BOLD when `needs_attention` (same as `sidebar::Row::style`);
    - `  ⎇ {branch} · {profile}` in DarkGray. When the branch is unknown, the line is `  {profile}`. When the profile is `None`, it reads `default`.
  - Overflow: when the entries do not fit in the inner height, the last visible line is `+N more`, where N is the number of sessions not shown.
  - Repo, branch and profile text goes through `clean`, and every line through `clip`.
- **Collapsed projects** are summarized in full (that is where the summary helps most). Projects with only remembered `Closed` sessions list them with `◌ closed`.
- **New pure module** `crates/baton/src/tui/project_summary.rs`. It has `Activity` (Work, Wait, Idle, Off), `fn activity(Status) -> Activity` and `fn lines(entries: &[Entry], width: usize, height: usize) -> Vec<Line<'static>>`. `Entry { n, info: &SessionInfo, branch: Option<&str> }`. This keeps the logic unit-testable without an `App`, as `info_panel.rs` is.
- **App accessor.** `App::summary_project() -> Option<&str>` returns the name only when the cursor is `Cursor::Project`. `App::project_sessions(name) -> Vec<&SessionInfo>` returns the sessions in `self.sessions` order, which is the order `sidebar::rows` numbers them in.
- **No new keys, config or protocol changes.**

### Task 1: Project summary model and lines
Language: rust
Files: crates/baton/src/tui/project_summary.rs (new), crates/baton/src/tui/mod.rs, crates/baton/src/tui/info_panel.rs (make `clean` and `clip` `pub(crate)`), crates/baton/src/tui/app.rs, crates/baton/src/tui/app/tests.rs
Depends on: none
Acceptance criteria:
- `project_summary::activity` maps statuses as follows, checked by a table test over all eight variants:
  - Starting, Running, Unknown → Work;
  - Permission, YourTurn → Wait;
  - Idle → Idle;
  - Exited(_), Closed → Off.
- `lines(&[], 30, 14)` returns exactly one line, `no sessions`.
- For three sessions (Running on branch `main` with profile `work`; Permission with no branch and profile `personal`; Idle on branch `feat/x` with profile `None`), `lines(.., 30, 14)` renders these texts:
  - `1 work · 1 wait · 1 idle`;
  - `1 ● api  running`;
  - `  ⎇ main · work`;
  - `2 ◐ web  permission`;
  - `  personal`;
  - `3 ○ cli  idle`;
  - `  ⎇ feat/x · default`.

  The `2 ◐ web  permission` line is fg Yellow + BOLD, and the branch/profile lines are fg DarkGray.
- Groups with zero sessions are left out of the tally. An Exited session counts as `off` and shows `✗ … exited N`.
- No line is wider than `width` (checked for width 10 and 30 with a long repo and branch). A branch containing `\x1b` or `\u{202e}` is shown without those characters.
- With 8 sessions and `height = 14`, the output has 14 lines, and the last one is `+2 more`. With 0 or 1 height, it does not panic.
- `App::summary_project()` returns `Some("x")` when the cursor is on project row `x`, and `None` on a session row and with no cursor. `App::project_sessions("x")` returns the project's sessions in sidebar order, including those of a collapsed project. Both are tested in `app/tests.rs`.
- `cargo test -p baton`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --check` pass.
Evidence: none — pure model and accessors; rendering is observable in Task 2.

### Task 2: Show the summary in the sidebar when a project row is under the cursor
Language: rust
Files: crates/baton/src/tui/ui.rs, docs/SPEC.md
Depends on: Task 1
Acceptance criteria:
- `draw_chrome` draws the lower sidebar block with the title `Project <name>` and the `project_summary::lines` content when `app.summary_project()` is `Some`. Each entry gets its branch from `app.branch(s)`, and the inner width and height of `info_area` are passed in. Otherwise it draws the `Session` block exactly as before.
- New ui test `project_row_shows_the_project_summary`: on `app_with_session()` with profile `p`, moving the cursor to project row `x` (press `k`) renders `Project x`, `1 work`, `1 ● a  running` and `  p` in the sidebar. The output no longer contains `Session` as a block title or `profile  p`.
- New ui test `session_row_keeps_the_session_info`: with the cursor on the session row, the render contains the `Session` title and `profile  p`, and no `Project x`. This means the existing info-panel tests keep passing unchanged.
- New ui test `collapsed_project_summary_lists_hidden_sessions`: with two sessions in `x`, `space` on the project row collapses it (`▸ x  (2)`), and the summary still lists `1 …` and `2 …`.
- New ui test `project_summary_shows_the_branch`: after `app.refresh_branches(|_| Some("main".into()))`, the summary contains `⎇ main · `.
- The main panel title and mirror are unchanged while the cursor is on a project row (the existing `a · p · ● running` assertion still holds).
- Fullscreen renders are unchanged (`fullscreen_has_no_pane_highlight` and the borderless tests pass).
- `docs/SPEC.md` §3 gains a **Project summary** bullet list (tally of work / wait / idle / off; per session: number, badge, repo, status, branch and profile, shown when the cursor is on a project row). §4 Layout gains a sentence saying the lower sidebar block shows `Project <name>` with that summary when the cursor is on a project row.
- `cargo test -p baton`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --check` pass.
Evidence: `cargo test -p baton tui::ui` passes the four new tests. In a real terminal, in `baton` with a project open, pressing `k` up to the `▾ <project>` row turns the lower sidebar block into `Project <project>`, with a tally line and a `⎇ branch · profile` line per session. Pressing `j` back onto a session restores the `Session` panel.

### Task 3: End-to-end check
Language: rust
Files: crates/baton/tests/e2e_keys.rs
Depends on: Task 2
Acceptance criteria:
- New e2e test `project_row_shows_a_summary_of_its_sessions`, using `Env::new("")`:
  1. Before opening, with the cursor on the closed project `x`, the screen shows `Project x` and `no sessions`.
  2. After `o`, it waits for `1 ○ alpha  idle` and `2 ○ beta  idle`.
  3. It presses `k` until the cursor is on `▾ x` (at most 3 presses). The screen then shows `Project x`, `2 idle`, a line containing `1 ○ alpha  idle` inside the lower block, and `  p` (the profile; the temp repos are not git repos, so no `⎇`).
  4. It presses `j`. The screen shows the `Session` title and `profile  p`.
- The test passes headless under `cargo test --locked`. The full gate (`cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test`) is green.
Evidence: `cargo test -p baton --test e2e_keys project_row_shows_a_summary` passes. `baton-drive --size 40x120 --step 'wait:idle' --step 'send:k' --step 'sleep:300' --step dump -- baton` (with a fake-claude config) shows a dump whose lower sidebar block is titled `Project x` and lists each session with its status and profile.
