# Evidence — Final: project-summary-tally-labels (whole feature)
Commit: 6d859d1
Environment: debug build (`target/debug/{baton,baton-drive,fake-claude}`, cargo package `baton-tui`); baton-drive PTY 40x120; fake-claude profile; short runtime dir `/tmp/bt.XXXX` (mode 700, verified `700`), BATON_RUNTIME_DIR/STATE_DIR/CONFIG inside it; `baton daemon stop` and `rm -rf` after each run. Screens below are trimmed (blank rows, Session-block rows cost/tokens/etc. and right-edge padding removed; baton-drive's own dump cuts the title line at a multibyte char).

## Goal: cursor on a project row shows the main-pane summary, tally uses the app's labels
Status: PROVEN
Two idle sessions, cursor moved to the project row (o, then k x3):
```console
$ baton-drive --size 40x120 --step 'wait:▸ x  \(closed\)' --step send:o --step 'wait:2 ○ beta  idle' --step send:k --step send:k --step send:k --step 'wait:2 idle' --step dump -- baton
┌Projects──────────────────────┐┌Project x─────────
│▾ x                           ││2 idle
│  1 ○ alpha  idle             ││1 ○ alpha  idle
│  2 ○ beta  idle              ││  p
│                              ││2 ○ beta  idle
│                              ││  p
```
Main pane titled "Project x" shows tally `2 idle`, same word as the sidebar and per-session lines.

Then a permission session and a your-turn session (alpha: `perm\r` sent via `baton debug send`; beta: `perm\r` then `allow\r`), cursor still on the project row, live update:
```console
$ baton-drive ... --step sleep:6000 --step 'wait:1 permission · 1 your turn' --step dump -- baton   (events sent from a second shell during the sleep)
a=0
b=0
┌Projects──────────────────────┐┌Project x─────────
│▾ x                           ││1 permission · 1 your turn
│  1 ◐ alpha  permission       ││1 ◐ alpha  permission
│  2 ✓ beta  your turn         ││  p
│                              ││2 ✓ beta  your turn
│                              ││  p
│status   permission
```
Tally reads `1 permission · 1 your turn` (fixed order, permission before your turn), matching sidebar labels and the per-session lines.

## Never work / wait / off
Status: PROVEN
```console
$ grep -nE '\b(work|wait|off)\b' <full baton-drive output of the run above>; echo grep_exit=$?
grep_exit=1
```
No match in any of the three dumped screens.

## Then back to a session
Status: PROVEN
```console
$ ... --step send:j --step sleep:800 --step dump
┌Projects──────────────────────┐┌alpha · p · ◐ permission
│▾ x                           ││DA1: ok
│  1 ◐ alpha  permission       ││FAKE CLAUDE session=6539d753-... model=claude-opus-5-5
│  2 ○ beta  idle              ││> perm
│                              ││permission needed
```
Main pane switches back to the session's own view (title "alpha · p · ◐ permission", session output). Side observation, not part of this feature: beta's `your turn` showed as `idle` here, consistent with core's `YourTurn + Viewed -> Idle` rule (status.rs:44); I did not investigate which action counted as viewing it.

## Evidence line: `cargo test -p baton-tui project_summary` and `--test e2e_keys`
Status: PROVEN
```console
$ cargo test -p baton-tui project_summary 2>&1 | grep -E "^test |test result: ok. 12"
test tui::project_summary::tests::group_words_match_status_labels ... ok
test tui::project_summary::tests::exited_sessions_are_counted_together ... ok
test tui::project_summary::tests::every_status_group_in_fixed_order ... ok
test tui::project_summary::tests::lines_are_clipped_and_cleaned ... ok
test tui::project_summary::tests::no_entries_say_no_sessions ... ok
test tui::project_summary::tests::overflow_ends_with_a_more_line ... ok
test tui::project_summary::tests::running_your_turn_idle_tally ... ok
test tui::project_summary::tests::three_sessions_render_tally_and_entries ... ok
test tui::app::tests::on_screen_is_none_while_a_project_summary_is_shown ... ok
test tui::ui::tests::project_summary_shows_the_branch ... ok
test tui::ui::tests::collapsed_project_summary_lists_hidden_sessions ... ok
test tui::ui::tests::project_row_shows_the_project_summary_in_the_main_pane ... ok
test result: ok. 12 passed; 0 failed
```
`e2e_keys` ran in the full gate below: `test result: ok. 5 passed; 0 failed` (includes `project_row_shows_a_summary_of_its_sessions`, the `2 idle` assertions).

## Running tally word
Status: NOT PROVEN live (covered by tests)
fake-claude's `prompt` goes straight through to Stop, so a session cannot be held in `running`. `1 running` is covered by `running_your_turn_idle_tally` and the ui.rs test above.

## Full gate
Status: PROVEN
```console
$ cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
gate_exit=0
(sum of all "test result" lines: 467 passed, 0 failed; no clippy warnings; e2e_keys: 5 passed)
```

## Cleanup
Temp dirs removed (`ls -d` confirmed gone), `baton daemon stop` printed `daemon stopped`. A pre-existing `target/release/baton daemon start --foreground` (pid 22870) was not started by me and was left alone.
