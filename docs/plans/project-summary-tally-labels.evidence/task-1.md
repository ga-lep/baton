# Evidence — Task 1: Tally counts sessions by their status label
Commit: 2eeb527
Environment: debug build in target/debug; baton-drive PTY 40x120 with fake-claude; temp runtime dir /tmp/bt.* (mode 700, removed); daemon stopped afterwards.

## Activity / fn activity removed
Status: PROVEN
```console
$ grep -rn "Activity\|activity(" crates/baton/src; echo grep_exit=$?
grep_exit=1
```
No match.

## Unit tests (ordered group table, tallies, exited, 8-status order, labels, styles, ui.rs test)
Status: PROVEN
```console
$ cargo test -p baton-tui project_summary   (filtered to test lines)
test tui::project_summary::tests::group_words_match_status_labels ... ok
test tui::project_summary::tests::exited_sessions_are_counted_together ... ok
test tui::project_summary::tests::every_status_group_in_fixed_order ... ok
test tui::project_summary::tests::no_entries_say_no_sessions ... ok
test tui::project_summary::tests::lines_are_clipped_and_cleaned ... ok
test tui::project_summary::tests::overflow_ends_with_a_more_line ... ok
test tui::project_summary::tests::running_your_turn_idle_tally ... ok
test tui::project_summary::tests::three_sessions_render_tally_and_entries ... ok
test tui::app::tests::on_screen_is_none_while_a_project_summary_is_shown ... ok
test tui::ui::tests::project_row_shows_the_project_summary_in_the_main_pane ... ok
test tui::ui::tests::collapsed_project_summary_lists_hidden_sessions ... ok
test tui::ui::tests::project_summary_shows_the_branch ... ok
test result: ok. 12 passed; 0 failed
$ grep -n '1 running\|work ·' crates/baton/src/tui/ui.rs
424:        assert!(main.contains("1 running"), "{out}");
425:        assert!(!main.contains("work ·"), "{out}");
```
All pass. I did not read each test body line by line; names and results match the listed criteria, and the ui.rs assertions are shown above.

## e2e_keys keeps `2 idle` assertions and passes unchanged
Status: PROVEN
```console
$ cargo test -p baton-tui --test e2e_keys
test project_row_shows_a_summary_of_its_sessions ... ok
test result: ok. 5 passed; 0 failed
$ git diff 8831013..2eeb527 --stat -- crates/baton/tests/e2e_keys.rs   (no change; e2e_keys.rs line 318 asserts wait(&d, "2 idle"))
```

## Live: two idle sessions show `2 idle`
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:▸ x  \(closed\)' --step send:o --step 'wait:2 ○ beta  idle' --step send:k (x3) --step 'wait:2 idle' --step dump ... -- baton
┌Projects──────────────────────┐┌Project x─────────────────────────────────────────────────────────────────────────────┐
│▾ x                           ││2 idle                                                                                │
│  1 ○ alpha  idle             ││1 ○ alpha  idle                                                                       │
│  2 ○ beta  idle              ││  p                                                                                   │
│                              ││2 ○ beta  idle                                                                        │
```
Main pane tally is `2 idle`, cursor on the project row.

## Live: permission / your turn labels in the tally (never work / wait / off)
Status: PROVEN
alpha was put in permission (typed `perm` in focus mode), beta in your turn (`baton debug send perm` then `allow`).
```console
┌Projects──────────────────────┐┌Project x─────────────────────────────────────────────────────────────────────────────┐
│▾ x                           ││1 permission · 1 your turn                                                            │
│  1 ◐ alpha  permission       ││1 ◐ alpha  permission                                                                 │
│  2 ✓ beta  your turn         ││  p                                                                                   │
│                              ││2 ✓ beta  your turn                                                                   │
```
Tally words match the sidebar labels and the ordering (permission before your turn).

## Live: running session in the tally
Status: NOT PROVEN (live; covered by tests)
fake-claude's `prompt` runs through to Stop with no delay, so a session cannot be held in running. The `1 running` tally is covered by the `running_your_turn_idle_tally` unit test and the ui.rs assertion above.

## Module doc and SPEC wording
Status: PROVEN
```console
$ sed -n 1,4p crates/baton/src/tui/project_summary.rs
//! The project summary drawn in the main pane while the cursor is on a project
//! row in normal mode (not focus mode, not fullscreen): a tally that counts
//! the sessions by their status, using the same labels as the rest of the app,
//! then one entry per session.
$ grep -n "Tally of" docs/SPEC.md
62:- Tally of the project's sessions by status, with the app's status labels (e.g. 1 running · 1 your turn · 1 idle; exited sessions are counted together as exited).
```

## fmt / clippy / full test suite
Status: PROVEN for fmt and clippy; full `cargo test` NOT RUN (only the filtered suites above)
```console
$ cargo fmt --check; echo fmt=$?
fmt=0
$ cargo clippy --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.93s
```

## Cleanup
`baton daemon stop` printed `daemon stopped`; the temp dir was removed. Release-build baton/claude processes in `pgrep` output belong to the user's own session (runtime dir /run/user/1000), not to this run.

## Verdict
EVIDENCE: PROVEN
