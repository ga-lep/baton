# Evidence — Task 2: Draw the summary in the main pane; sidebar back to Session
Commit: fdde08b
Environment: cargo (debug build), package baton-tui. TUI driven by target/debug/baton-drive (40x120 PTY) against target/debug/baton with a scratch config (fake-claude profile `p`, project `x` with repos alpha and beta), BATON_RUNTIME_DIR under /tmp (short, mode 700), BATON_NOTIFY_SINK=off. Daemon stopped afterwards with `baton daemon stop`; scratch dir removed. Screen excerpts are trimmed to the first rows and the bottom bar; long rows cut at 110 columns. Full tests-and-drive run was a single baton-drive invocation (exit=0).

## ui tests pass (`cargo test -p baton-tui tui::ui`)
Status: PROVEN
```console
$ cargo test -p baton-tui tui::ui
test tui::ui::tests::project_row_shows_the_project_summary_in_the_main_pane ... ok
test tui::ui::tests::collapsed_project_summary_lists_hidden_sessions ... ok
test tui::ui::tests::project_summary_shows_the_branch ... ok
test tui::ui::tests::session_row_keeps_the_session_info ... ok
test tui::ui::tests::main_title_and_info_panel_describe_the_session ... ok
test tui::ui::tests::focus_mode_never_shows_the_summary ... ok
test tui::ui::tests::fullscreen_has_no_pane_highlight ... ok
test tui::ui::tests::fullscreen_draws_only_the_mirror_and_the_bar ... ok
(28 ui tests listed in the run, all ok; 8 shown)
test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 189 filtered out
```
All the tests named in the criteria exist and pass. fmt/clippy and the full test suite were not rerun here.

## Real TUI: `draw` shows the summary in the main pane on a project row; main title `Project <name>`; session view otherwise
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'send:o' --step 'wait:2 ○ beta|2 . beta' --step 'wait:FAKE CLAUDE' --step 'sleep:500' --step dump \
    --step 'send:k' --step 'sleep:500' --step dump --step 'send:r' --step 'sleep:300' --step dump \
    --step 'send:j' --step 'wait:FAKE CLAUDE' --step 'sleep:300' --step dump -- target/debug/baton
exit=0
[screen 1: after opening, cursor on session alpha]
┌Projects──────────────────────┐┌alpha · p · ○ idle───────────────────────────────────────────────────────────
│▾ x                           ││DA1: ok
│  1 ○ alpha  idle             ││FAKE CLAUDE session=739582fb-b80e-4bbf-9a4f-92e708c91f7d model=claude-opus-5-
│  2 ○ beta  idle              ││>
[screen 2: after k, cursor on project row ▾ x]
┌Projects──────────────────────┐┌Project x────────────────────────────────────────────────────────────────────
│▾ x                           ││2 idle
│  1 ○ alpha  idle             ││1 ○ alpha  idle
│  2 ○ beta  idle              ││  p
│                              ││2 ○ beta  idle
│                              ││  p
```
Before `k` the main pane is titled with the session and mirrors the terminal; after `k` it is titled `Project x` with the tally `2 idle` and one entry per session (status and profile), and the mirror text is gone.

## Sidebar lower block is always `Session` with info_text
Status: PROVEN
```console
[screen 2 (cursor on project row), sidebar lower block, columns 1-32, copied from the screen 2 dump]
┌Session───────────────────────┐
│/tmp/b2tamt/alpha             │
│profile  p                    │
│model    n/a                  │
│status   idle                 │
```
On the project row the lower sidebar block is still titled `Session` and shows the last shown session (`/tmp/.../alpha`, `profile  p`, `status   idle`), not a project summary.

## Session-targeting key on a project row hints `select a session first`
Status: PROVEN
```console
[screen 3: after `r` on the project row]
┌Projects──────────────────────┐┌Project x────────────────────────────────────────────────────────────────────
 NORMAL │ select a session first
```
The bar shows the hint and the main pane still reads `Project x` (no restart happened).

## Terminal mirror returns after `j`
Status: PROVEN
```console
[screen 4: after j]
┌Projects──────────────────────┐┌alpha · p · ○ idle───────────────────────────────────────────────────────────
│▾ x                           ││DA1: ok
│  1 ○ alpha  idle             ││FAKE CLAUDE session=739582fb-b80e-4bbf-9a4f-92e708c91f7d model=claude-opus-5-
 NORMAL │ ⏎ focus  n next-attention  r restart ...
```
The session title and the live `FAKE CLAUDE` mirror are back; `Project x` is gone.

## Closed project (extra observation)
Status: PROVEN
```console
[first baton-drive attempt, project closed, cursor on `▸ x  (closed)`]
┌Projects──┐┌Project x──...
│▸ x  (closed) ││no sessions
┌Session───┐ ... │no session
```
The summary shows `no sessions` in the main pane and the sidebar block is `Session`.

## docs/SPEC.md edits
Status: NOT PROVEN (documentation; not observable from outside)

## fmt / clippy
Status: NOT PROVEN (not run in this evidence pass)

## Cleanup
`baton daemon stop` printed `daemon stopped`; no fake-claude from the scratch dir remained. A pre-existing `target/release/baton daemon` (pid 22870) is not mine and was left alone.
