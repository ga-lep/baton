# Evidence — Task 4: Doctor version line
Commit: 08c0a56
Environment: real `target/debug/baton` (cargo build), `doctor --no-probe`, scratch config/state dir (0700) outside the repo, runtime dir `/tmp/b4r` (0700; short path needed because the scratch path exceeded SUN_LEN, which made an earlier run print `FAIL daemon: path must be shorter than SUN_LEN`, unrelated to the feature). Profile command is the repo's `fake-claude`. Fake release server: python http.server on 127.0.0.1:18765 that appends one line per request to a log (`connections=N` = lines). `BATON_UPDATE_URL` points at it; `GH_TOKEN`/`BATON_NO_UPDATE_CHECK` unset unless stated. `<scratch>` abbreviates the scratch dir. Output is `tail`ed to the last lines of doctor (the version line is the last line, after notify). Daemon is not running, so the only non-version lines omitted are PASS/WARN ones.

Raw output of the main run (script `run.sh`, unedited):

```console
### A: BATON_NO_UPDATE_CHECK=1
PASS version: DA1: timeout (profile p)
PASS notify: notify-send and the D-Bus session bus are available
PASS version: 0.1.0 (update check disabled)
exit=0
connections=0
### B: update_check=false
PASS notify: notify-send and the D-Bus session bus are available
PASS version: 0.1.0 (update check disabled)
exit=0
connections=0
### C: fresh ok cache newer
PASS notify: notify-send and the D-Bus session bus are available
WARN version: 99.0.0 is available (you have 0.1.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0
exit=0
connections=0
### D: fresh failed cache
PASS notify: notify-send and the D-Bus session bus are available
WARN version: could not check for updates (last check failed)
exit=0
connections=0
### E: stale cache
{"checked_at": 1791463777, "latest": "v0.0.5", "html_url": "https://github.com/ga-lep/baton/releases/tag/v0.0.5", "etag": null, "ok": true}
PASS notify: notify-send and the D-Bus session bus are available
WARN version: 99.0.0 is available (you have 0.1.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0
exit=0
connections=1
{"checked_at":1791553777,"latest":"v99.0.0","html_url":"https://github.com/ga-lep/baton/releases/tag/v99.0.0","etag":"\"abc\"","ok":true}
-rw------- 1 glepape glepape 137 Oct  9 15:49 <scratch>/state/update-check.json
### F: rerun, cache now fresh
WARN version: 99.0.0 is available (you have 0.1.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0
connections=0
### G: unreachable
PASS notify: notify-send and the D-Bus session bus are available
WARN version: could not check for updates (io: Connection refused (os error 111))
exit=0

real	0m0.013s
user	0m0.003s
sys	0m0.003s
### G2 disabled timing baseline

real	0m0.004s
user	0m0.002s
sys	0m0.003s
```

Raw output of the second run (up to date / hanging server / full output):

```console
### H: up to date
PASS notify: notify-send and the D-Bus session bus are available
PASS version: 0.1.0 (latest)
exit=0
### I: server accepts, never answers
PASS notify: notify-send and the D-Bus session bus are available
WARN version: could not check for updates (timeout: global)
exit=0

real	0m3.074s
### J: full output, hint command (BATON_UPDATE_URL=http://127.0.0.1:9/)
PASS config: <scratch>/config.toml parses
PASS dirs: runtime dir /tmp/b4r is private (0700)
PASS dirs: state dir <scratch>/state is writable
WARN daemon: not running
PASS command: /home/glepape/project/baton/target/debug/fake-claude (profile p)
PASS version: DA1: timeout (profile p)
PASS notify: notify-send and the D-Bus session bus are available
WARN version: could not check for updates (io: Connection refused (os error 111))
exit=0
```

## Doctor prints exactly one `version:` line after the notify check, with the four wordings; never FAIL
Status: PROVEN
Sections C, E (WARN `99.0.0 is available (you have 0.1.0): <url>`), H (`PASS version: 0.1.0 (latest)`), D/G/I (`WARN version: could not check for updates (...)`), A/B (`PASS version: 0.1.0 (update check disabled)`) show all four wordings, each as the last line after `notify:`, exit 0 in every case. Note: a separate pre-existing `PASS version: DA1: timeout (profile p)` line (the profile command's `--version`, printed by the command check before notify) also begins with `version:`; the new line is the only one after notify and is the only one from this task.

## Fresh cache (ok: true, newer latest): no connection, WARN with cached URL
Status: PROVEN
Section C: `WARN version: 99.0.0 is available (you have 0.1.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0`, `connections=0`, exit=0.

## Stale cache with server answering: exactly one connection, cache rewritten
Status: PROVEN
Section E: cache before had `checked_at` 90000 s old, `latest: v0.0.5`; after, `connections=1`, cache is `latest: v99.0.0`, new `checked_at`, etag `"abc"`, mode `-rw-------`. Section F: a re-run then makes `connections=0`.

## Fresh failed cache: no connection, `WARN version: could not check for updates (last check failed)`
Status: PROVEN
Section D: exact line printed, `connections=0`, exit=0.

## `BATON_NO_UPDATE_CHECK=1` or `update_check = false`: `PASS version: <ver> (update check disabled)`, no connection
Status: PROVEN
Section A (env) and B (config key): `PASS version: 0.1.0 (update check disabled)`, `connections=0` both, exit=0.

## Unreachable BATON_UPDATE_URL: WARN, exit 0, within usual time + 3 s
Status: PROVEN
Section G (`http://127.0.0.1:9/`): `WARN version: could not check for updates (io: Connection refused (os error 111))`, exit=0, real 0.013 s (disabled baseline 0.004 s). Worst case, section I (server accepts and never answers): WARN `(timeout: global)`, exit=0, real 3.074 s, i.e. baseline + 3 s as specified.

## Existing `e2e_doctor` tests set `BATON_NO_UPDATE_CHECK=1` and keep their assertions
Status: PROVEN
```console
$ cargo test -p baton --test e2e_doctor
running 12 tests
... (all listed ok, including the pre-existing invalid_config_fails, no_probe_skips_the_hook_check, healthy_fake_claude_profile_passes_every_check, hooks_that_never_fire_fail_the_probe_with_a_hint)
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.03s
```
The test harness `doctor_with` sets `.env("BATON_NO_UPDATE_CHECK", "1")` by default (crates/baton/tests/e2e_doctor.rs line 40); the diff only adds helpers and new tests.

## Verdict
EVIDENCE: PROVEN
