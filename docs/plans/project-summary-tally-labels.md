# Project Summary Tally Labels Plan
Status: APPROVED

## Goal
Small follow-up to `docs/plans/project-summary-pane.md` and `docs/plans/project-summary-main-pane.md`, which are both implemented. The project summary's tally line uses its own words (`work` / `wait` / `idle` / `off`, from the private `Activity` grouping in `crates/baton/src/tui/project_summary.rs`). The rest of the app uses the status labels from `labels::status_label` (`starting`, `running`, `permission`, `your turn`, `idle`, `exited N`, `unknown`, `closed`). The user decided the tally must use the app's own labels and count sessions by their real status, e.g. `1 running · 1 your turn · 1 idle`. The groups appear in this fixed order, and only groups with a non-zero count are shown: running, permission, your turn, idle, starting, unknown, exited, closed. Every `Exited(code)` session counts in a single `exited` group with no code (`2 exited`), because the per-session line just below the tally already shows `exited N` for each one. The `Activity` enum and the `activity()` mapping are then removed. They are only used inside `project_summary.rs` (checked with grep: `ui.rs` uses only `project_summary::Entry` and `project_summary::lines`). The yellow+bold style on session lines that need attention stays. It comes from `baton_core::attention::needs_attention`, not from `Activity`. No protocol, daemon, config or layout changes.

## Findings (read from the repo on 2026-10-18)
- `project_summary.rs`: the only users of `Activity`, `Activity::ALL` and `activity()` are `tally()` and the unit test `activity_maps_every_status`. `entry_lines` styles the head line with `needs_attention(s)`, which does not depend on them.
- `labels::status_label` returns `String` and embeds the code for `Exited`. The tally needs a group word without the code, so the group table holds its own words. These are the same strings as `status_label`, except `exited` has no code. A unit test checks that each word matches `status_label` for that status (for `Exited`, that `status_label(Exited(n))` starts with the word), so the two lists cannot drift apart.
- Tests that assert the old words:
  - `project_summary.rs` tests: `"1 work · 1 wait · 1 idle"` and `"1 off"`.
  - `ui.rs:424`: `main.contains("1 work")`.
  - `crates/baton/tests/e2e_keys.rs:318` and `:348` assert `2 idle` (two idle sessions). These still hold unchanged.
- `docs/SPEC.md` §3, "Project summary" bullet: `Tally of the project's sessions: work / wait / idle / off.`

### Task 1: Tally counts sessions by their status label
Language: rust
Files: crates/baton/src/tui/project_summary.rs, crates/baton/src/tui/ui.rs, crates/baton/tests/e2e_keys.rs (check only, no change expected), docs/SPEC.md
Depends on: none
Acceptance criteria:
- `Activity`, `Activity::ALL` and `fn activity` are removed from `project_summary.rs`, and `grep -rn "Activity\|activity(" crates/baton/src` finds no match for them.
- The tally is built from an ordered group table: running, permission, your turn, idle, starting, unknown, exited, closed. Each group matches its `Status` (with `Exited(_)` matching every exit code). Groups with zero sessions are left out, and the shown groups are joined with ` · `.
- Unit test: running + permission + idle sessions render the tally `1 running · 1 permission · 1 idle`. The existing entry and style assertions stay (the permission head line is yellow+bold, detail lines are DarkGray).
- Unit test: running + your turn + idle sessions render `1 running · 1 your turn · 1 idle`.
- Unit test: `Exited(2)` and `Exited(0)` sessions give the tally `2 exited`, and the entry lines still read `exited 2` / `exited 0`.
- Unit test: one session of each of the 8 statuses, given in a shuffled order, gives `1 running · 1 permission · 1 your turn · 1 idle · 1 starting · 1 unknown · 1 exited · 1 closed`.
- Unit test: every group word equals `status_label` for its status (for exited: `status_label(Exited(n))` starts with `exited`).
- Unit test: a `YourTurn` session's head line is still yellow+bold, and an `Idle` session's head line has the default style.
- `ui.rs` test `project_row_shows_the_project_summary_in_the_main_pane` asserts `1 running` instead of `1 work`, and asserts that `work ·` does not appear in the main pane.
- `e2e_keys.rs` keeps its `2 idle` assertions and passes without changes.
- The module doc of `project_summary.rs` says the tally counts sessions by their status, using the same labels as the rest of the app.
- `docs/SPEC.md` §3 reads: `Tally of the project's sessions by status, with the app's status labels (e.g. 1 running · 1 your turn · 1 idle; exited sessions are counted together as exited).`
- `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test` passes (including `cargo test -p baton-tui`).
Evidence: `cargo test -p baton-tui project_summary` and `cargo test -p baton-tui --test e2e_keys` pass. With two idle sessions, put the cursor on a project row in normal mode (as the `e2e_keys` project-summary test does through `baton-drive`): the main pane shows the tally `2 idle`. A session that is running or waiting for the user shows up in the tally as `running` / `permission` / `your turn`, never as `work` / `wait` / `off`.
