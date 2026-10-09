# Evidence — Task 5: Non-blocking TUI notice; hook/statusline/daemon never check
Commit: 9967149
Environment: real `target/debug/baton` + `target/debug/baton-drive` (cargo build, up to date). Scratch dir outside the repo (config with one profile `p` = bash, one project `x`), per-scenario `BATON_STATE_DIR` (0700), `BATON_RUNTIME_DIR=/tmp/b5r` (0700), `BATON_NOTIFY_SINK=off`, `BATON_NO_UPDATE_CHECK` unset unless stated. Fake release server: python socket server (modes ok = 200 with `v99.0.0`, 404, hang = accept and never answer, slow = answer after 1.5 s) appending one line per accepted connection to a log (`connections=N`). TUI launched as `baton-drive --size 30x120 ... -- sh -c 'exec target/debug/baton 2>$S/err-<name>'`, so the TUI's own stderr goes to a file separate from the PTY screen, and baton-drive's own stderr is captured separately. Screens were filtered (`grep`/`cut`) to the relevant lines; full runs are described in prose. All servers/daemons I started were stopped; the user's release daemon (pid 25713) was not touched.

## At TUI startup: fresh cache with newer latest shows ` v<latest> available ` without any connection
Status: PROVEN
```console
### fresh   (cache {"checked_at":<now>,"latest":"v99.0.0",...,"ok":true}, fake server up)
1:--- screen 30x120 ---                       <- first dump, right after the sidebar appears
14:└─────────── v99.0.0 available ┘│
31: NORMAL │ ⏎ focus  n next-attention ...
33:--- screen 30x120 ---                       <- dump 2.5 s later
46:└─────────── v99.0.0 available ┘│
connections=0 stderr_bytes=0
```
The notice is on the Projects block's bottom border in the very first dump, and the server saw 0 connections.

## Stale cache starts one background fetch; first frame is drawn before it completes
Status: PROVEN
```console
### stale   (cache checked_at = now-90000 s, latest v0.0.5; fake server "slow" answers after 1.5 s)
1:--- screen 30x120 ---                       <- first dump: sidebar drawn, no notice
31: NORMAL │ ⏎ focus  n next-attention ...
32:--- end screen ---
33:--- screen 30x120 ---                       <- after 2.5 s
46:└─────────── v99.0.0 available ┘│
connections=1 stderr_bytes=0
```
First frame appears with no notice while the server is still delaying; the notice arrives later; exactly 1 connection. (The 2 s timeout itself is shown by the hung-server run below, where no cache is written because the process quit first, and by the passing `hung_server_times_out` test.)

## A disabled check (env or config) reads nothing and fetches nothing
Status: PROVEN
```console
### envoff  (BATON_NO_UPDATE_CHECK=1, no cache)
 ... NORMAL ... (no "available" line in either dump)
connections=0 stderr_bytes=0
### cfgoff  (config.toml with top-level `update_check = false`, no cache)
 ... NORMAL ... (no "available" line in either dump)
connections=0 stderr_bytes=0
```
Both disable paths made 0 connections. "Reads nothing" is covered by the code path (`startup()` returns before the cache read) and by tests `disabled_tui_does_not_connect` / `broken_config_disables_automatic_checks`, not observed from outside.

## e2e: fake server returning v99.0.0 -> screen contains `v99.0.0 available`, cache file written
Status: PROVEN
```console
$ baton-drive --size 30x120 --step wait:Projects --step sleep:1500 --step 'wait:v99.0.0 available' --step dump --step send:q --step expect-exit:0 -- sh -c 'exec target/debug/baton 2>$S/err-ok'
drive exit=0
14:└─────────── v99.0.0 available ┘│
--- tui stderr file: 0 bytes     --- baton-drive stderr: (empty)
--- connections: 1
--- cache:
{"checked_at":1791554835,"latest":"v99.0.0","html_url":"https://github.com/ga-lep/baton/releases/tag/v99.0.0","etag":null,"ok":true}
```
(`html_url` is read from the fake server's JSON; the exact string comes from the hardened URL sanitiser accepting the pinned prefix.)

## e2e: 404 -> no notice, no error text, stderr empty, cache ok:false
Status: PROVEN
```console
(same drive command, fake server in 404 mode)
drive exit=0
 top line of screen: ┌Projects──────────────────────┐┌Baton───...   bottom border of Projects block: plain └──────┘ (no title)
 screen has no "available", no "error", no "no public release" text
--- tui stderr file: 0 .../err-n404     --- baton-drive stderr: (empty)
--- connections: 1
--- cache:
{"checked_at":1791554822,"latest":null,"html_url":null,"etag":null,"ok":false}
```
The full dump showed only the normal sidebar/"No sessions" panes and the NORMAL status line. TUI stderr (redirected to a file) is 0 bytes.

## e2e: accept-and-never-answer server -> sidebar drawn, quit within 1 s, no error, stderr empty
Status: PROVEN
```console
### hang  (steps: wait:Projects, sleep:500, wait:Projects, dump, send:q, expect-exit:0)
drive exit=0 elapsed=.547291742          <- whole run incl. the 500 ms sleep
screen: sidebar + NORMAL status line, no error text
--- tui stderr file: 0 bytes     --- baton-drive stderr: (empty)
--- connections: 1                (fetch was in flight, hung, when q was sent)
### baseline (BATON_NO_UPDATE_CHECK=1, identical steps)
baseline exit=0 elapsed=.546859868
```
Total run with the hung fetch in flight is 0.547 s versus 0.547 s with checks disabled, i.e. q-to-exit adds no measurable delay (the whole run including startup and the 500 ms sleep is far under 1 s). No cache existed afterwards because the process exited before the 2 s timeout, as designed.

## Unit test on `ui::draw` shows the notice for Some and nothing for None
Status: PROVEN
```console
$ cargo test -p baton --bin baton ui::
test tui::ui::tests::update_notice_is_shown_only_when_available ... ok
test result: ok. 99 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

## e2e_update_paths: hook, statusline, daemon start/stop, config check, version -> 0 connections, silent, no cache file
Status: PROVEN
```console
$ echo '{}' | baton hook Stop        -> (no output) exit=0
$ echo '{}' | baton statusline       -> (no output) exit=0
$ baton daemon start                 -> daemon already running pid=176867 (spawned earlier by my TUI runs in /tmp/b5r) exit=0
$ baton daemon stop                  -> daemon stopped, exit=0
$ baton config check                 -> x  .../a  profile=p cmd=[...]  exit=0
$ baton version                      -> baton 0.1.0 (protocol 6), exit=0
connections=0
$ ls $S/state-hook
ls: cannot access '.../state-hook': No such file or directory     (state dir and update-check.json never created)

$ cargo test -p baton --test e2e_update_paths --test e2e_version
test hook_statusline_daemon_config_and_version_never_connect ... ok
test tui_shows_notice_from_fresh_cache_without_connecting ... ok
test tui_shows_notice_from_fetch_and_writes_cache ... ok
test tui_is_silent_on_404 ... ok
test tui_does_not_wait_for_a_hung_server ... ok
test tui_ignores_group_writable_cache_file ... ok
test disabled_tui_does_not_connect ... ok
test broken_config_disables_automatic_checks ... ok
test result: ok. 8 passed; 0 failed ...      (e2e_version: 18 passed, 0 failed)
```
Caveat: my manual `daemon start` found a daemon already running (left by an earlier TUI run in my scratch runtime dir), so the manual run did not exercise a fresh start; the passing repo test does exercise start/stop. Manual run proves 0 connections and no state dir.

## Hardening (extra): untrusted cache, broken config
Status: PROVEN (by repo tests only)
`tui_ignores_group_writable_cache_file`, `planted_cache_in_untrusted_state_dir_is_ignored`, `broken_config_disables_automatic_checks`, `foreign_github_url_is_dropped`, `hostile_server_data_never_reaches_the_terminal` all pass (output above); not re-exercised by hand.

## `baton hook Stop` with no daemon returns in under 1 s
Status: PROVEN
```console
$ time (echo '{}' | baton hook Stop; echo "exit=$?")
exit=0
real	0m0.002s
$ echo '{}' | BATON_UPDATE_URL=http://127.0.0.1:1/ baton hook Stop; echo $?
0
```
Only `0` printed (no stdout from the hook), in 2 ms.

## Verdict
All criteria proven.
