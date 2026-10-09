# Evidence — Final: ci-release-update-check (whole feature)
Commit: aaf0a46 (artifacts and CI from GitHub runs at 4effafc; later commits touched docs/plans only)
Environment: scratch dir /tmp/bfin2.XXXX (0700, removed afterwards); runtime dir inside it (0700); fake GitHub server = python http.server on 127.0.0.1 (random port, counts hits, switchable 200/500, ETag `"abc123"`, html_url https://github.com/ga-lep/baton/releases/tag/v99.0.0). Installed binary = musl archive from Release build-only run 37951159917. `BATON_CONFIG`/`BATON_STATE_DIR`/`BATON_RUNTIME_DIR` all in the scratch dir. The user's ~/.local/state/baton and daemon (pid 25713) were not touched; the daemon spawned by `hook Stop` was stopped with `baton daemon stop`; fake server killed; scratch dir removed. Nothing pushed, tagged, merged or dispatched.
Note: the artifact is named baton-517d10d36f7d-… (GitHub's PR merge-commit sha of the run), not 4effafc.

## GitHub CI and Release runs (reused from 4effafc)
Status: PROVEN
```console
$ gh run view 37951160469 --repo ga-lep/baton | head -8
✓ feat/ci-release-update-check CI ga-lep/baton#4 · 37951160469
JOBS
✓ gate in 1m10s (ID 113889983689)

$ gh run view 37951159917 --repo ga-lep/baton | head -14
✓ feat/ci-release-update-check Release ga-lep/baton#4 · 37951159917
JOBS
✓ gate / gate in 1m33s (ID 113889985203)
✓ verify in 12s (ID 113890688827)
✓ build (x86_64-unknown-linux-gnu) in 1m17s (ID 113890800632)
✓ build (x86_64-unknown-linux-musl) in 2m5s (ID 113890800764)
- publish (ID 113891716820)
ARTIFACTS
baton-x86_64-unknown-linux-musl
baton-x86_64-unknown-linux-gnu

$ gh run view <id> --repo ga-lep/baton --json conclusion,headSha,event -q .     (both runs)
{"conclusion":"success","event":"pull_request","headSha":"4effafc1a32932a85b65d69fa3b4c62c3bcb8f0e"}
{"conclusion":"success","event":"pull_request","headSha":"4effafc1a32932a85b65d69fa3b4c62c3bcb8f0e"}

$ gh run view 37951160469 --repo ga-lep/baton --log | grep -c 'test result: ok'
26
```
CI gate and Release gate/verify/builds are green at 4effafc; publish is skipped (PR event, no tag). CI log shows 26 `test result: ok`, equal to the local count in step 6 below.

## Step 1: install as a user from the release artifacts (README Install section)
Status: PROVEN

README.md, quoted: "1. Download the `x86_64-unknown-linux-musl` archive (recommended) from GitHub Releases, together with its checksum file. 2. Verify it, in the download directory, with either `sha256sum -c baton-<tag>-x86_64-unknown-linux-musl.tar.gz.sha256` or `sha256sum -c SHA256SUMS --ignore-missing`. ... 3. Extract it and put `baton` on your `PATH`, for example in `~/.local/bin`."
```console
$ gh run download 37951159917 --repo ga-lep/baton -D $D/art      (needed retries: TLS handshake timeouts on this network; last attempt exit 0)
$ find $D/art -type f | sort
art/baton-x86_64-unknown-linux-gnu/baton-517d10d36f7d-x86_64-unknown-linux-gnu.tar.gz
art/baton-x86_64-unknown-linux-gnu/baton-517d10d36f7d-x86_64-unknown-linux-gnu.tar.gz.sha256
art/baton-x86_64-unknown-linux-musl/baton-517d10d36f7d-x86_64-unknown-linux-musl.tar.gz
art/baton-x86_64-unknown-linux-musl/baton-517d10d36f7d-x86_64-unknown-linux-musl.tar.gz.sha256
$ cd $D/art/baton-x86_64-unknown-linux-musl
$ tar tzf *.tar.gz
baton-517d10d36f7d-x86_64-unknown-linux-musl/
baton-517d10d36f7d-x86_64-unknown-linux-musl/baton
baton-517d10d36f7d-x86_64-unknown-linux-musl/LICENSE
baton-517d10d36f7d-x86_64-unknown-linux-musl/README.md
$ sha256sum -c baton-*-x86_64-unknown-linux-musl.tar.gz.sha256
baton-517d10d36f7d-x86_64-unknown-linux-musl.tar.gz: OK
$ tar xzf *.tar.gz -C $D/x && cp $D/x/*/baton $D/bin/baton
$ head -1 $D/x/*/LICENSE
MIT License
$ $D/bin/baton --version; echo exit=$?
baton 0.2.0
exit=0
$ file $D/bin/baton
/tmp/bfin2.OXJM/bin/baton: ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), static-pie linked, BuildID[sha1]=bcb6f910a473bc2170dd8ecfde1fc284bbbf0014, stripped
```
Checksum OK, archive holds baton/LICENSE/README.md, `MIT License`, `baton 0.2.0`, static-pie.

## Step 2: `version` offline, `version --check` against the real API and a fake server
Status: PROVEN
```console
$ BATON_UPDATE_URL=http://127.0.0.1:<fake>/ baton version; echo exit=$?
baton 0.2.0 (protocol 6)
exit=0
fake hits: 0

$ env -u BATON_UPDATE_URL BATON_STATE_DIR=$D/state2 baton version --check; echo exit=$?     (REAL api.github.com, repo public, no release)
could not check for updates: no public release found
exit=1
$ cat $D/state2/update-check.json
{"checked_at":1791560181,"latest":null,"html_url":null,"etag":null,"ok":false}

$ BATON_UPDATE_URL=http://127.0.0.1:<fake>/ baton version --check; echo exit=$?     (fake: 200, v99.0.0)
baton 99.0.0 is available (you have 0.2.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0
exit=0
fake hits: 1
$ ls -ld $D/state; ls -l $D/state; cat $D/state/update-check.json
drwx------ 2 glepape glepape 4096 Oct  9 17:36 /tmp/bfin2.OXJM/state
-rw------- 1 glepape glepape 140 Oct  9 17:36 update-check.json
{"checked_at":1791560175,"latest":"v99.0.0","html_url":"https://github.com/ga-lep/baton/releases/tag/v99.0.0","etag":"\"abc123\"","ok":true}
$ (request headers seen by the fake)
/ {'host': '127.0.0.1:52443', 'accept': 'application/vnd.github+json', 'x-github-api-version': '2022-11-28', 'user-agent': 'baton/0.2.0'}
```
`version` makes 0 connections; real API gives `no public release found`, exit 1; fake gives the available line with the link (exit 0); cache 0600 in a 0700 dir; headers as specified, no Authorization.

## Step 3: failing server, doctor WARN, opt-out
Status: PROVEN
```console
$ (fake switched to 500) baton version --check; echo exit=$?
could not check for updates: HTTP 500
exit=1
$ cat $D/state/update-check.json
{"checked_at":1791560175,"latest":"v99.0.0","html_url":"https://github.com/ga-lep/baton/releases/tag/v99.0.0","etag":"\"abc123\"","ok":false}

$ baton doctor --no-probe | cat -A | grep -E 'version|exit'      (output of the run, "exit" line is my echo)
WARN version: could not check for updates (last check failed)$
exit=0$

$ BATON_NO_UPDATE_CHECK=1 baton doctor --no-probe | grep -E 'version|exit'
PASS version: 0.2.0 (update check disabled)
exit=0

$ baton doctor --no-probe | grep version      (later, cache ok again after the TUI fetch)
WARN version: 99.0.0 is available (you have 0.2.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0
```
Failure keeps `latest`/`etag` in the cache (`ok:false`); the doctor WARN has no trailing ": " (`$` end-of-line marker directly after `)`); the available WARN ends with the URL; opt-out gives the PASS line; doctor exit 0.

## Step 4: TUI notice
Status: PROVEN
```console
$ BATON_UPDATE_URL=<fake> baton-drive --size 30x120 --step 'wait:v99.0.0 available' --step dump --step 'send:q' --step 'expect-exit:0' -- $D/bin/baton     (state dir empty before; screen trimmed to first 14 lines + footer)
--- screen 30x120 ---
┌Projects──────────────────────┐┌Baton─────────────────────────────────────────────────────────────────────────────────┐
│                              ││No sessions. Start a project with `baton debug open <name>`.                          │
  ... (empty rows)
└─────────── v99.0.0 available ┘│                                                                                      │
┌Session───────────────────────┐│                                                                                      │
  ...
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  ? help  q quit
--- end screen ---
exit status of baton-drive (expect-exit:0 satisfied): 0
fake hits: 1
```
Notice shown as the right-aligned bottom title of the Projects block, `q` exits 0, one fetch.

## Step 5: silent paths
Status: PROVEN
```console
$ echo '{}' | baton hook Stop; echo exit=$?     (stdout/stderr redirected to files)
exit=0 stdout_bytes=0 stderr_bytes=0
$ echo '{}' | baton statusline; echo exit=$?
exit=0 stdout_bytes=0 stderr_bytes=0
fake hits: 0
$ ls $D/state
ls: cannot access '/tmp/bfin2.OXJM/state': No such file or directory
$ baton daemon stop      (cleanup of the daemon spawned by hook Stop)
daemon stopped
```
Both print nothing, exit 0, no connection, no state dir/cache created.

## Step 6: proxy independence (known finding from the previous final.md is fixed)
Status: PROVEN
```console
$ HTTP_PROXY=http://127.0.0.1:9 http_proxy=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9 cargo test --locked --no-fail-fast > log; echo exit=$?
exit=0
$ grep -c '^test result: ok' log
26
$ grep '^test result:' log | grep -vc 'ok\.'
0
$ grep '^test result:' log | awk '{p+=$4; f+=$6} END{print "passed="p" failed="f}'
passed=416 failed=0
```
26 `test result: ok` lines (same as the CI log), 0 non-ok, 416 passed / 0 failed with proxy variables set (previously 15 failures).

## Step 7: license
Status: PROVEN
```console
$ cargo metadata --no-deps --format-version 1 | jq -r '.packages[].license' | sort -u
MIT
```
(`head -1 LICENSE` of the shipped archive = `MIT License`, step 1.)

## Per-task Evidence lines
Status: PROVEN (task-1.md ... task-7.md and final-fixes.md in this directory; user-facing ones, version, doctor, TUI notice, silent hook, license, release artifacts, were re-run above with the released binary at this commit)

## Pending (user decision)
- Publishing v0.2.0 (tag + release; `gh release view v0.2.0 --json assets` listing 2 archives, 2 .sha256, SHA256SUMS): not done. Status: NOT PROVEN (pending; needs a real release).
- `baton version --check` reporting `baton 0.2.0 is up to date` against the real GitHub release: pending the above; today it prints `no public release found`.
- PR #4 remains draft; nothing pushed, tagged, merged or dispatched.

## Verdict
EVIDENCE: PROVEN (publishing v0.2.0 and the real "up to date" check are pending the user's decision)
