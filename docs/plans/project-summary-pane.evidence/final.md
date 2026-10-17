# Evidence — Final: project-summary-pane (whole feature)
Commit: ce1f522
Environment: debug build; real `target/debug/baton` driven by `target/debug/baton-drive --size 40x120` (PTY). Short runtime dir `/tmp/bs.yc7i/run` (mode 700), fake-claude for profiles `p` and `work`. Project `x` (profile p) = repos `alpha` (git, branch main) + `beta` (plain dir); project `y` (profile work) = repo `gamma` (git, branch feat/y). Cargo package is `baton-tui`. Screens cut to 62 columns, blank panel rows removed. The daemon I started was stopped (`baton daemon stop`) and the temp dir removed; unrelated pre-existing release baton/claude processes under /run/user/1000 were not touched.

## Goal: cursor on a closed project shows `Project x` / `no sessions`
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:Project x' --step dump ... -- target/debug/baton
┌Projects─────────────────
│▸ x  (closed)                 ││No sessions. Start a 
│▸ y  (closed)                 ││                     
┌Project x────────────────
│no sessions                   ││
```

## Goal: open sessions, then cursor on session row shows `Session` panel (unchanged)
Status: PROVEN
```console
$ ... --step send:o --step 'wait:2 ○ beta  idle' --step dump
│▾ x                           ││DA1: ok
│  1 ○ alpha  idle             ││FAKE CLAUDE session=1
│  2 ○ beta  idle              ││>
┌Session─────────────────
│/tmp/bs.yc7i/alpha            ││
│profile  p                    ││
│status   idle                 ││
```

## Goal: cursor to project row (`k`) -> tally, per-session number/badge/repo/status, branch and profile; main panel unchanged
Status: PROVEN
```console
$ ... --step send:k --step dump
│▾ x                           ││DA1: ok
│  1 ○ alpha  idle             ││FAKE CLAUDE session=1
┌Project x────────────────
│2 idle                        ││
│1 ○ alpha  idle               ││
│  ⎇ main · p                  ││
│2 ○ beta  idle                ││
│  p                           ││
```
Title `Project x`, tally `2 idle`, git repo alpha shows `⎇ main · p`, non-git beta shows just `  p`. The main panel mirror is the same as before `k`.

## Goal: back to a session row (`j`) restores `Session`
Status: PROVEN
```console
$ ... --step send:j --step dump
┌Session─────────────────
│/tmp/bs.yc7i/alpha            ││
│profile  p                    ││
│status   idle                 ││
```

## Goal: collapsed project is summarized in full
Status: PROVEN
```console
$ ... --step send:k --step 'send: ' --step dump     (collapse x)
│▸ x  (2)                      ││DA1: ok
│▸ y  (closed)                 ││FAKE CLAUDE session=1
┌Project x────────────────
│2 idle                        ││
│1 ○ alpha  idle               ││
│  ⎇ main · p                  ││
│2 ○ beta  idle                ││
│  p                           ││
```
The hidden sessions of the collapsed `▸ x  (2)` are listed with branch and profile.

## Goal: project with a git branch and a non-default profile (second project)
Status: PROVEN
```console
$ (second baton-drive run attached to the same daemon) --step send:j --step send:j --step dump
│▾ y                           ││
│  1 ○ gamma  idle             ││
┌Project y────────────────
│1 idle                        ││
│1 ○ gamma  idle               ││
│  ⎇ feat/y · work             ││
```
Branch `feat/y` and profile `work` appear. (In the first run, after opening y with the cursor on its row, `k` moved to x and showed `Project x`, as expected for that cursor position.) Both runs exited 0 on `q`.

## Task 1 (Evidence: none; unit tests): activity table, lines, overflow, accessors
Status: PROVEN via tests (internal); the renders above show them from outside
```console
$ cargo test --locked -p baton-tui
test tui::project_summary::tests::activity_maps_every_status ... ok
test tui::project_summary::tests::exited_counts_as_off_and_empty_groups_are_left_out ... ok
test tui::project_summary::tests::lines_are_clipped_and_cleaned ... ok
test tui::project_summary::tests::no_entries_say_no_sessions ... ok
test tui::project_summary::tests::overflow_ends_with_a_more_line ... ok
test tui::project_summary::tests::three_sessions_render_tally_and_entries ... ok
test tui::app::tests::summary_project_is_set_only_on_a_project_row ... ok
test tui::app::tests::project_sessions_follow_sidebar_order_even_when_collapsed ... ok
test result: ok. 214 passed; 0 failed   (all other result lines: ok, 0 failed)
```
`cargo fmt --check` printed nothing (ok); `cargo clippy --locked --all-targets --all-features -- -D warnings` finished with no warnings.

## Task 2 Evidence: four ui tests pass; SPEC updated
Status: PROVEN
```console
$ cargo test --locked -p baton-tui   (filtered)
test tui::ui::tests::collapsed_project_summary_lists_hidden_sessions ... ok
test tui::ui::tests::project_row_shows_the_project_summary ... ok
test tui::ui::tests::project_summary_shows_the_branch ... ok
test tui::ui::tests::session_row_keeps_the_session_info ... ok
$ grep -n "Project summary\|Project <name>" docs/SPEC.md
61:**Project summary** (cursor on a project row)
103:`Project <name>` with the project summary (see §3) instead of `Session`.
```
The real-terminal part of this Evidence line (`k` to project row, `j` back) is shown in the sections above.

## Task 3 Evidence: e2e test passes; baton-drive dump
Status: PROVEN
```console
$ cargo test --locked -p baton-tui   (filtered)
test project_row_shows_a_summary_of_its_sessions ... ok
test result: ok. 5 passed; 0 failed
```
The baton-drive dump with `Project x` listing sessions with status and profile is shown above. The full `cargo test` run had no failures.

## Verdict
EVIDENCE: PROVEN
