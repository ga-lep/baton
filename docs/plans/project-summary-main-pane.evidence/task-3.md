# Evidence — Task 3: End-to-end check
Commit: 821a085
Environment: cargo debug build, package baton-tui. TUI driven by target/debug/baton-drive (40x120) against target/debug/baton with a scratch config (fake-claude profile `p`, project `x`, repos alpha/beta) in a /tmp dir (mode 700), BATON_RUNTIME_DIR under it, BATON_NOTIFY_SINK=off. Daemon stopped with `baton daemon stop`; scratch dir removed. Screen dumps trimmed: long rows cut at 110 columns, blank rows dropped. Unrelated pre-existing processes (a release-build baton daemon and claude sessions of the user) were left alone.

## `project_row_shows_a_summary_of_its_sessions` rewritten for the main pane, steps 1-5; passes and is stable
Status: PROVEN
```console
$ for i in 1..5: cargo test --locked -p baton-tui --test e2e_keys project_row_shows_a_summary
test project_row_shows_a_summary_of_its_sessions ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.36s
(identical result on all 5 runs)
```
Test passed 5 of 5 runs.

Real TUI, same steps (closed project, `o`, `k`, `r`, `j`):
```console
[screen 1: cursor on closed project x]
┌Projects──────────────────────┐┌Project x──────
│▸ x  (closed)                 ││no sessions
[screen 2: after o, k]
│▾ x                           ││2 idle
│  1 ○ alpha  idle             ││1 ○ alpha  idle
│  2 ○ beta  idle              ││  p
│                              ││2 ○ beta  idle
┌Session───────────────────────┐│
[screen 3: after r]
 NORMAL │ select a session first
[screen 4: after j]
┌Projects──────────────────────┐┌alpha · p · ○ idle─
│  1 ○ alpha  idle             ││FAKE CLAUDE session=8fe32a9e-... model=claude
```
Matches: `Project x`/`no sessions` before open; `Project x` + `2 idle` and `1 ○ alpha  idle` / `  p` (no ⎇) in the main pane after `k`; sidebar still has `Session`; `r` gives "select a session first"; `j` shows FAKE CLAUDE and `alpha · p · `, no `Project x`. (baton-drive exit=0.)

## Other e2e tests pass unchanged; full gate green
Status: PROVEN
```console
$ cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
gate=0
(all "test result" lines ok, 0 failed; e.g. 119, 15, 7, 217, 15, 8, 5, 3 passed)
```
The diff touches only crates/baton/tests/e2e_keys.rs; the whole gate exits 0.

## Evidence line: baton-drive dump shows main pane `Project x` listing sessions with status and profile next to `Session` sidebar
Status: PROVEN
See screen 2 above: main pane titled `Project x`, each session with status and profile `p`, sidebar block `Session` beside it.
