# Evidence — Task 3: End-to-end check
Commit: db66caa
Environment: debug build; real `target/debug/baton` driven by `target/debug/baton-drive --size 40x120` (PTY), fake-claude as profile `p`, project `x` with plain-dir repos `alpha` and `beta`, scratch config/state/runtime dirs under a short `/tmp/bs.*` path (removed afterwards, the daemon the run spawned was killed). Env: BATON_CONFIG, BATON_STATE_DIR, BATON_RUNTIME_DIR, BATON_NO_UPDATE_CHECK=1, BATON_NOTIFY_SINK=off. Dump output cut to 60 columns and blank panel rows removed. Cargo package is `baton-tui` (the plan's `-p baton` does not resolve).

## New e2e test `project_row_shows_a_summary_of_its_sessions` passes headless under `cargo test --locked`
Status: PROVEN
```console
$ cargo test --locked -p baton-tui --test e2e_keys project_row_shows_a_summary
running 1 test
test project_row_shows_a_summary_of_its_sessions ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.46s
```
The test passes. The full gate (fmt/clippy/test) was not run by me: NOT PROVEN here.

## 1. Closed project under cursor shows `Project x` and `no sessions`
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:▸ x' --step 'wait:Project x' --step dump ... -- target/debug/baton
│▸ x  (closed)                 ││No sessions. Start 
┌Project x────────────────
│no sessions                   ││
```
Both strings are on screen before opening.

## 2. After `o`: `1 ○ alpha  idle` and `2 ○ beta  idle`
Status: PROVEN
```console
(--step send:o --step 'wait:2 ○ beta  idle'; dump after k)
│▾ x                           ││DA1: ok
│  1 ○ alpha  idle             ││FAKE CLAUDE session
│  2 ○ beta  idle              ││>
```
The wait step succeeded and both lines are in the sidebar.

## 3. After `k` (cursor on `▾ x`): `Project x`, `2 idle`, session line in the lower block, `  p`, no `⎇`
Status: PROVEN
```console
┌Project x────────────────
│2 idle                        ││
│1 ○ alpha  idle               ││
│  p                           ││
│2 ○ beta  idle                ││
│  p                           ││
```
The lower block is titled `Project x`, has tally `2 idle`, lists each session with status and profile `p`, and has no `⎇` (non-git dirs). One `k` was enough.

## 4. After `j`: `Session` title and `profile  p`
Status: PROVEN
```console
┌Session────────────────
│/tmp/bs.5XbY/alpha            ││
│profile  p                    ││
│status   idle                 ││
```
`q` then exited with code 0 (`exit=0`). The block reverts to the Session panel.
