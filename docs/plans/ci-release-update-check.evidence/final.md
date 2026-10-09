# Evidence — Final: ci-release-update-check (whole feature)
Commit: 0a82c59 (artifacts from CI/Release runs at 714888c; later commits touched docs/plans only)
Environment: scratch dir under the session scratchpad, runtime dir /tmp/bfin (0700), fake GitHub server = python http.server on 127.0.0.1:18765 returning `{"tag_name":"v99.0.0","html_url":"http://x/r"}` and counting hits. Installed binary = musl archive downloaded from Release run 37947724799. User's daemon/state untouched; my daemon (spawned by `hook Stop`) was stopped with `baton daemon stop`; fake server killed; /tmp/bfin removed.
Note: the artifact is named baton-27aa08146698-… (GitHub's PR merge-commit sha of the run), not 714888c.

## GitHub CI and Release runs
Status: PROVEN
```console
$ gh run view 37947724254 --repo ga-lep/baton | head -8

✓ feat/ci-release-update-check CI ga-lep/baton#4 · 37947724254
Triggered via pull_request about 10 minutes ago

JOBS
✓ gate in 1m12s (ID 113878247432)

For more information about the job, try: gh run view --job=113878247432
$ gh run view 37947724799 --repo ga-lep/baton | head -16

✓ feat/ci-release-update-check Release ga-lep/baton#4 · 37947724799
Triggered via pull_request about 10 minutes ago

JOBS
✓ gate / gate in 1m2s (ID 113878250281)
✓ verify in 14s (ID 113878717631)
✓ build (x86_64-unknown-linux-musl) in 1m32s (ID 113878838877)
✓ build (x86_64-unknown-linux-gnu) in 1m14s (ID 113878839281)
- publish in 0s (ID 113879529053)

ARTIFACTS
baton-x86_64-unknown-linux-musl
baton-x86_64-unknown-linux-gnu

For more information about a job, try: gh run view --job=<job-id>
```
CI gate green; Release gate, verify and both builds green; publish skipped (PR event, no tag).

## Chain, steps 1-5 (install as a user, version, version --check real+fake, doctor, TUI, silent paths)
Status: PROVEN
```console
## 1
$ sha256sum -c
baton-27aa08146698-x86_64-unknown-linux-musl.tar.gz: OK
$ head -1 LICENSE
MIT License
$ $D/bin/baton --version
baton 0.2.0
exit=0

$D/bin/baton: ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), static-pie linked, BuildID[sha
## 2
$ env -u BATON_UPDATE_URL $D/bin/baton version
baton 0.2.0 (protocol 6)
exit=0

$ real API
could not check for updates: no public release found
exit=1

$ $D/bin/baton version
baton 0.2.0 (protocol 6)
exit=0

fake hits after version: 0
$ $D/bin/baton version --check
baton 99.0.0 is available (you have 0.2.0)
exit=0

fake hits: 2
$ ls -ld state; ls -l state; cat cache
drwx------ 2 glepape glepape 4096 Oct  9 17:04 $D/state
total 4
-rw------- 1 glepape glepape 82 Oct  9 17:04 update-check.json
{"checked_at":1791558275,"latest":"v99.0.0","html_url":null,"etag":null,"ok":true}
## 3
$ $D/bin/baton doctor --no-probe
PASS config: $D/config.toml parses
PASS dirs: runtime dir /tmp/bfin is private (0700)
PASS dirs: state dir $D/state is writable
WARN daemon: not running
PASS notify: notify-send and the D-Bus session bus are available
WARN version: 99.0.0 is available (you have 0.2.0): 
exit=0

$ env BATON_NO_UPDATE_CHECK=1 $D/bin/baton doctor --no-probe
PASS config: $D/config.toml parses
PASS dirs: runtime dir /tmp/bfin is private (0700)
PASS dirs: state dir $D/state is writable
WARN daemon: not running
PASS notify: notify-send and the D-Bus session bus are available
PASS version: 0.2.0 (update check disabled)
exit=0

## 4
$ baton-drive
--- screen 30x120 ---
┌Projects──────────────────────┐┌Baton─────────────────────────────────────────────────────────────────────────────────┐
│                              ││No sessions. Start a project with `baton debug open <name>`.                          │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
└─────────── v99.0.0 available ┘│                                                                                      │
┌Session───────────────────────┐│                                                                                      │
│no session                    ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
│                              ││                                                                                      │
└──────────────────────────────┘└──────────────────────────────────────────────────────────────────────────────────────┘
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  ? help  q quit
--- end screen ---
exit=0
fake hits: 3
## 5
$ echo {} | baton hook Stop
exit=0
$ echo {} | baton statusline
exit=0
fake hits: 0
ls: cannot access '$D/state': No such file or directory
$ baton daemon stop   (cleanup of the daemon spawned by hook Stop)
daemon stopped
```
Proves: checksum OK; LICENSE+README.md in the archive and `MIT License`; `baton 0.2.0`; static-pie musl binary; `version` makes 0 connections; against the real API (no release yet) `no public release found`, exit 1; fake server -> "99.0.0 is available (you have 0.2.0)", exit 0, cache 0600 in 0700 dir; doctor WARN with cache and `PASS version: 0.2.0 (update check disabled)` with BATON_NO_UPDATE_CHECK=1; TUI shows `v99.0.0 available` on the Projects block and `q` exits 0; `hook Stop` and `statusline` print nothing, exit 0, fake server sees 0 hits and no state dir was created.
Observation (by design, not a defect): the fake's `html_url` is `http://x/r`, a non-GitHub URL, so it is dropped (cache `html_url":null`, no URL after the colon in the `available` line; doctor line ends `(you have 0.2.0): `). The plan's `foreign_github_url_is_dropped` and `newer_without_url_has_no_dangling_separator` tests cover this. The `version --check` line has no dangling separator; the doctor WARN line above does end with `: ` and a trailing space when the URL is absent. This is cosmetic and worth a look.

## Step 6: license
Status: PROVEN
```console
$ cargo metadata --no-deps --format-version 1 --locked | jq -r ".packages[].license" | sort -u
MIT
```

## Per-task Evidence lines
Status: PROVEN (see task-1.md ... task-7.md in this directory; the user-facing ones, version, doctor, TUI notice and silent hook, were re-run above with the released binary)

## Known finding: update-check e2e tests fail when proxy env vars are set
Status: FAILED (known, not fixed; reported by the final code review)
```console
$ HTTP_PROXY=http://127.0.0.1:9 http_proxy=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9 cargo test -q --locked --no-fail-fast -p baton --test e2e_version --test e2e_doctor --test e2e_update_paths 2>&1 | grep "test result"
test result: FAILED. 11 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.04s   (e2e_doctor)
test result: FAILED. 5 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 15.13s   (e2e_update_paths)
test result: FAILED. 8 passed; 10 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s   (e2e_version)

$ (same command with all proxy variables unset)
test result: ok. 13 passed; 0 failed; ...  (e2e_doctor)
test result: ok. 8 passed; 0 failed; ...   (e2e_update_paths)
test result: ok. 18 passed; 0 failed; ...  (e2e_version)
```
Cause (per review): ureq honours proxy env and the test helpers do not clear it, so requests to the 127.0.0.1 fake go to the proxy. Environment-only; the shipped binary is unaffected. (The per-suite attribution of result lines is by test names in the failure list; the lines are quoted from the run, the suite labels and the proxy-free "ok" lines are my abbreviation of the grep output.)

## Pending (user decision)
- Publishing v0.2.0 (tag + release): not done. Status: NOT PROVEN (pending; needs a real release).
- `baton version --check` reporting "up to date" against the real GitHub release: pending the above; today it prints `no public release found`. Covered locally by `check_reports_up_to_date` (passes without proxy env).
- PR #4 remains draft; nothing pushed, tagged, merged or dispatched.

## Verdict
EVIDENCE: PROVEN (with one known finding: proxy-env test failures; one cosmetic observation on the doctor WARN line)
