# Evidence — Task 2: Show the summary in the sidebar when a project row is under the cursor
Commit: a1e2f3f
Environment: real `target/debug/baton` driven by `target/debug/baton-drive --size 30x120` (PTY), fake-claude as profile `p`, scratch config/state/runtime dirs outside the repo, project `x` with repos `alpha` (a `git init -b main` repo) and `beta` (plain dir). Cargo package is `baton-tui` (the plan's `-p baton` does not resolve). Screens cut to 120 columns; blank lines in the lower block trimmed nowhere, whole sidebar shown. Daemon and temp dirs started by the run were stopped/removed.

## New ui tests pass (project_row_shows_the_project_summary, session_row_keeps_the_session_info, collapsed_project_summary_lists_hidden_sessions, project_summary_shows_the_branch)
Status: PROVEN
```console
$ cargo test -p baton-tui tui::ui
test tui::ui::tests::collapsed_project_summary_lists_hidden_sessions ... ok
test tui::ui::tests::project_summary_shows_the_branch ... ok
test tui::ui::tests::project_row_shows_the_project_summary ... ok
test tui::ui::tests::session_row_keeps_the_session_info ... ok
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 187 filtered out; finished in 0.02s
```
All four named tests pass (output filtered to those lines plus the result line).

## Cursor on a project row: lower block titled `Project <name>` with tally and per-session lines (branch, profile); main panel unchanged
Status: PROVEN
```console
$ baton-drive --size 30x120 --step wait:Projects --step send:o --step 'wait:2 ○ beta  idle' --step sleep:500 --step dump --step send:k --step sleep:400 --step dump --step send:j --step sleep:400 --step dump --step send:q -- target/debug/baton
(dump 2, after `k`; sidebar columns and top of main title)
┌Projects──────────────────────┐┌alpha · ⎇ main · p · ○ idle─
│▾ x                           ││DA1: ok
│  1 ○ alpha  idle             ││FAKE CLAUDE session=... model=claude-opus-5-5
│  2 ○ beta  idle              ││>
...
┌Project x─────────────────────┐│
│2 idle                        ││
│1 ○ alpha  idle               ││
│  ⎇ main · p                  ││
│2 ○ beta  idle                ││
│  p                           ││
│                              ││
```
After `k` the lower block is `Project x` with tally `2 idle`, each session line, `⎇ main · p` for the git repo alpha and just `  p` for non-git beta. The main panel title (`alpha · ⎇ main · p · ○ idle`) and mirror are identical to the pre-`k` dump.

## Cursor on a session row: `Session` panel exactly as before
Status: PROVEN
```console
(dump 1, after opening, cursor on session; and dump 3, after `j`)
┌Session───────────────────────┐│
│…c949d2dcb/scratchpad/t2/alpha││
│profile  p                    ││
│model    n/a                  ││
│status   idle                 ││
...
```
Dump 1 (before `k`) and dump 3 (after `j`) both show the `Session` title and `profile  p`; no `Project x`. Pressing `j` restores the Session panel.

## Fullscreen renders unchanged; main panel title/mirror unchanged; unit gates
Status: PROVEN (fullscreen via existing tests only)
```console
$ cargo test -p baton-tui
test result: ok. 214 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ cargo fmt --check; echo fmt=$?
fmt=0
$ cargo clippy --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
```
Whole baton-tui suite (including fullscreen tests) is green; fmt and clippy clean. Fullscreen was not driven manually.

## docs/SPEC.md §3 Project summary list and §4 Layout sentence
Status: PROVEN
```console
$ git diff b9667bc..a1e2f3f -- docs/SPEC.md | grep '^[+-]'
+**Project summary** (cursor on a project row)
+- Tally of the project's sessions: work / wait / idle / off.
+- Per session: number, badge, repo, status, git branch and profile.
+
+When the cursor is on a project row, the lower sidebar block shows
+`Project <name>` with the project summary (see §3) instead of `Session`.
+
```
Both required additions are present.

## Note
The plan's `cargo test -p baton` does not resolve (package is `baton-tui`); not a feature defect.
