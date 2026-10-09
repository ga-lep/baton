# Evidence — Final-review fixes (ci-release-update-check)
Commit: 4effafc
Environment: local Linux, debug build (`cargo build --locked --workspace --bins`), fake release server (python http.server on 127.0.0.1:18765) via BATON_UPDATE_URL, scratch dir /tmp/bfx (0700, removed afterwards), own daemon in /tmp/bfx/run stopped with `baton daemon stop`. The user's daemon (pid 25713) and ~/.local/state/baton were not touched. Output below is condensed from the real runs; the run logs are summarised, not edited.

## 1. Update-check e2e tests no longer depend on proxy env
Status: PROVEN
```console
$ HTTP_PROXY=http://127.0.0.1:9 http_proxy=... HTTPS_PROXY=... ALL_PROXY=http://127.0.0.1:9 cargo test --locked --no-fail-fast -p baton --test e2e_version --test e2e_doctor --test e2e_update_paths
     Running tests/e2e_doctor.rs
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.03s
     Running tests/e2e_update_paths.rs
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.57s
     Running tests/e2e_version.rs
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.07s
$ cargo test --locked --workspace            -> every "test result" line: ok, 0 failed
$ BATON_NO_UPDATE_CHECK=1 cargo test --locked --workspace   -> every "test result" line: ok, 0 failed
```
All three suites pass with a dead proxy configured; full workspace passes plain and with the check disabled.

## 2. `baton doctor` no dangling separator
Status: PROVEN
```console
$ # 0700 state dir, 0600 update-check.json {"latest":"v99.0.0","html_url":null,"ok":true,...}
$ baton doctor --no-probe | grep -i version | cat -A
WARN version: --version printed nothing (profile p)$
WARN version: 99.0.0 is available (you have 0.2.0)$
$ ... | od -c (tail)
... 0.2.0)  \n          (no ": " before the newline)

$ # fake server, html_url=https://example.com/evil (rejected as non-github)
$ baton version --check 2>&1 | cat -A
baton 99.0.0 is available (you have 0.2.0)$
$ baton doctor --no-probe | grep available | cat -A
WARN version: 99.0.0 is available (you have 0.2.0)$
```
(A second `version --check` in the first attempt hit a bug in my fake server and returned the valid github URL: `...(you have 0.2.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0$`, showing the ": url" suffix only appears when a URL exists. The repeat above, with a corrected server, is the bad-URL run.)

## 3. Transient failure keeps the known release
Status: PROVEN
```console
$ # server mode ok (200, ETag "e1", v99.0.0)
$ baton version --check; echo exit=$?
baton 99.0.0 is available (you have 0.2.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0
exit=0
BEFORE: {"checked_at":1791559222,"latest":"v99.0.0","html_url":"https://github.com/ga-lep/baton/releases/tag/v99.0.0","etag":"\"e1\"","ok":true}
$ # server mode 500
$ baton version --check; echo exit=$?
could not check for updates: HTTP 500
exit=1
AFTER-500: {"checked_at":1791559222,"latest":"v99.0.0","html_url":"https://github.com/ga-lep/baton/releases/tag/v99.0.0","etag":"\"e1\"","ok":false}
server log: [ok] GET /r If-None-Match=None
            [500] GET /r If-None-Match="e1"

$ baton-drive --size 30x120 --step 'wait:v99\.0\.0 available' --step dump --step 'send:q' -- target/debug/baton   (cache ok:false as above)
14:└─────────── v99.0.0 available ┘│    ...

$ # server mode 304
$ baton version --check; echo exit=$?
baton 99.0.0 is available (you have 0.2.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0
exit=0
{"checked_at":1791559245,"latest":"v99.0.0","html_url":"https://github.com/ga-lep/baton/releases/tag/v99.0.0","etag":"\"e1\"","ok":true}
server log: [304] GET /r If-None-Match="e1"
```
latest/html_url/etag survive the 500 with ok:false, the TUI still shows `v99.0.0 available`, the 500 retry carried If-None-Match, and 304 restores ok:true.

## 4. ci.yml uses `rustup toolchain install`
Status: PROVEN
```console
$ git push origin feat/ci-release-update-check
   714888c..4effafc  feat/ci-release-update-check -> feat/ci-release-update-check
$ gh run watch 37951160469 --repo ga-lep/baton --exit-status   # CI
run 37951160469 exit=0   (gate: success, headSha 4effafc1...)
$ gh run watch 37951159917 --repo ga-lep/baton --exit-status   # Release (build-only, pull_request)
run 37951159917 exit=0   (gate, verify, build gnu, build musl: success; publish: skipped)
$ gh run view 37951160469 --log | grep "Install toolchain"
gate	Install toolchain	2026-10-09T15:21:52.5495772Z ##[group]Run rustup toolchain install
gate	Install toolchain	...  rustup toolchain install
gate	Install toolchain	...  rustup show
```

## 5. RELEASING.md wording
Status: PROVEN
```console
$ sed -n 18,20p / 28,30p / 91,93p docs/RELEASING.md
`.github/workflows/release.yml`. A tag with a suffix such as `v0.2.0-rc.1`
publishes a pre-release. The `Cargo.toml` version must carry the same suffix
(for example `0.3.0-rc.1`) before you tag `v0.3.0-rc.1`, because `verify`
   smoke-tests each binary (`--version` and `baton version`) and uploads the
   archives. For musl the smoke test also requires `file` to report
   `statically linked` or `static-pie linked`, and `readelf` to show no
   `INTERP` program header and no `NEEDED` entries.
The check needs a public release to exist. Until one does, `baton version
--check` reports `no public release found`, `baton doctor` shows a WARN, and
the TUI shows nothing, the same as when offline.
```

## 6. README.md
Status: PROVEN (with one note)
```console
7: ... [docs/SPEC.md](https://github.com/ga-lep/baton/blob/main/docs/SPEC.md)
16:   `sha256sum -c baton-<tag>-x86_64-unknown-linux-musl.tar.gz.sha256` or
17:   `sha256sum -c SHA256SUMS --ignore-missing`. Checksums prove the download
18:   is intact, not who built it.
21: See [docs/RELEASING.md](https://github.com/ga-lep/baton/blob/main/docs/RELEASING.md)
$ gh api repos/ga-lep/baton/contents/docs/SPEC.md?ref=main               -> docs/SPEC.md
$ gh api .../docs/RELEASING.md?ref=feat/ci-release-update-check         -> docs/RELEASING.md
$ gh api .../docs/RELEASING.md?ref=main                                  -> 404 Not Found
$ curl -sI https://github.com/ga-lep/baton/blob/main/docs/SPEC.md       -> HTTP/2 200
$ curl -sI https://github.com/ga-lep/baton/blob/main/docs/RELEASING.md  -> HTTP/2 404
```
Note: the RELEASING.md link targets `main`, where the file does not exist yet; it resolves on the branch and will resolve once PR #4 is merged. Not a defect of the change, but the link is dead until merge.

## Verdict
EVIDENCE: PROVEN
