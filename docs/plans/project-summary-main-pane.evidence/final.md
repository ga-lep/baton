# Evidence — Whole feature: Project Summary in the Main Pane
Commit: de8b7b4
Environment: debug build (`cargo build --locked -p baton-tui --bins`), `target/debug/baton-drive --size 40x120` driving `target/debug/baton`, fake-claude profile `p`. Scratch dir `/tmp/bt.XXXX` (mode 700) with BATON_RUNTIME_DIR/STATE_DIR/CONFIG under it, BATON_NOTIFY_SINK=off. Projects: `x` (repos alpha, beta, no git) and `g` (one git repo `gr` on branch `feat-z`). Daemon stopped with `baton daemon stop`; no process left; scratch dir removed. Screen dumps trimmed: blank/border-only rows dropped, rows cut at 105 columns, some Session-panel rows (context..launch) dropped.

## Chain: closed project -> open sessions -> cursor to project row (summary in MAIN pane, sidebar `Session`)
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:closed' --step dump --step 'send:o' --step 'wait:2 ○ beta  idle' --step 'send:k' --step 'sleep:400' --step dump ... -- baton
[closed, cursor on x]
┌Projects──────────────────────┐┌Project x─────────
│▸ x  (closed)                 ││no sessions
│▸ g  (closed)                 │
┌Session───────────────────────┐│
│no session                    │
[after o, k]
│▾ x                           ││2 idle
│  1 ○ alpha  idle             ││1 ○ alpha  idle
│  2 ○ beta  idle              ││  p
│▸ g  (closed)                 ││2 ○ beta  idle
│                              ││  p
┌Session───────────────────────┐│
│/tmp/bt.qGOK/alpha            │
│profile  p                    │
│status   idle                 │
```
Main pane is titled `Project x` with tally and per-session entries; lower sidebar block stays `Session`; closed project shows `no sessions`.

## `r` on the project row shows the hint; `j` brings the mirror back
Status: PROVEN
```console
[after r]
 NORMAL │ select a session first
(Session panel: uptime 0s -> 1s, same session, not restarted; main pane still `Project x`)
[after j]
┌Projects──────────────────────┐┌alpha · p · ○ idle─────
│▾ x                           ││DA1: ok
│  1 ○ alpha  idle             ││FAKE CLAUDE session=b8ad5f4b-6915-4f3c-b922-674d52906ca3 model=claude-opus-5-5
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor ...
```
Hint shown, no restart, and the session's live terminal returned with no `Project x`.

## Collapsed project
Status: PROVEN
```console
[after space on x, cursor on x]
│▸ x  (2)                      ││Project x───
│▸ g  (closed)                 ││2 idle
                               ││1 ○ alpha  idle
                               ││  p
                               ││2 ○ beta  idle
                               ││  p
```
Sidebar shows `▸ x  (2)` while the main pane lists both hidden sessions.

## Project with a git branch shows `⎇ branch`
Status: PROVEN
```console
[cursor on project g]
│▾ g                           ││Project g───
│  1 ○ gr  idle                ││1 idle
                               ││1 ○ gr  idle
                               ││  ⎇ feat-z · p
```
Branch of the real git repo appears in the main-pane entry (sidebar Session block unchanged).

## Focus mode / fullscreen still show the session
Status: PROVEN
```console
[Enter on session gr, typed "hello"]
┌Projects───...┐┌gr · ⎇ feat-z · p · ○ idle──
                ││> hello█
 FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session  Alt-z fullscreen
[z on session gr (fullscreen), then k]
DA1: ok
FAKE CLAUDE session=1a80bc3e-56f6-48da-bfd1-f5c70a5572ce model=claude-opus-5-5
>
 NORMAL │ ⏎ focus  n next-attention  r restart ...
(identical after k: still the session mirror, no `Project`)
```
Focus mode mirrors the focused session and fullscreen still draws the session mirror even after `k`. (Whether `k` landed on the project row in fullscreen is not visible since the sidebar is hidden; unit test `summary_project_is_set_only_on_a_project_row` covers that case.)

## Task 1 Evidence: none (internal)
Status: NOT PROVEN (internal — covered by tests)
```console
$ cargo test --locked -p baton-tui tui::
test tui::app::tests::on_screen_is_none_while_a_project_summary_is_shown ... ok
test tui::app::tests::session_keys_on_a_project_row_only_hint ... ok
test tui::app::tests::summary_project_is_set_only_on_a_project_row ... ok
test result: ok. 141 passed; 0 failed
```
`on_screen` reporting (daemon-side effect) is only observable through the unit tests above.

## Task 2 Evidence: `cargo test -p baton-tui tui::ui` and real-terminal walkthrough
Status: PROVEN
```console
$ cargo test --locked -p baton-tui tui::ui
test tui::ui::tests::project_row_shows_the_project_summary_in_the_main_pane ... ok
test tui::ui::tests::collapsed_project_summary_lists_hidden_sessions ... ok
test tui::ui::tests::project_summary_shows_the_branch ... ok
test tui::ui::tests::session_row_keeps_the_session_info ... ok
test tui::ui::tests::main_title_and_info_panel_describe_the_session ... ok
test tui::ui::tests::focus_mode_never_shows_the_summary ... ok
test tui::ui::tests::fullscreen_has_no_pane_highlight ... ok
test result: ok. 28 passed; 0 failed
```
The real-terminal part is the chain sections above (`k` -> `Project <name>`, `Session` kept, `r` hint, `j` back).

## Task 3 Evidence: `e2e_keys project_row_shows_a_summary` and baton-drive dump
Status: PROVEN
```console
$ cargo test --locked -p baton-tui --test e2e_keys
test project_row_shows_a_summary_of_its_sessions ... ok
test result: ok. 5 passed; 0 failed
$ cargo fmt --check; echo fmt=$?     -> fmt=0
$ cargo clippy --locked --all-targets --all-features -- -D warnings   -> Finished, no warnings
$ cargo test --locked    -> every "test result" line ok, 0 failed (119, 217, 19, 15, 11, ...)
```
The baton-drive dump matches the Task 3 Evidence line (see chain section: `Project x`, status and profile per session, `Session` block beside it). Full gate is green.
