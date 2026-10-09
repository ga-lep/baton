# Evidence — Task 1: CI gate workflow and toolchain pin
Commit: 177334d
Environment: local rustc 1.97.1 (8bab26f4f 2026-07-14); actionlint via docker rhysd/actionlint:latest; GitHub via gh as ga-lep. Branch pushed to origin, draft PR https://github.com/ga-lep/baton/pull/4 opened.

## rust-toolchain.toml pins 1.97.1 with rustfmt and clippy; local gate passes
Status: PROVEN
```console
$ cat rust-toolchain.toml
[toolchain]
channel = "1.97.1"
components = ["rustfmt", "clippy"]
$ cargo fmt --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --no-fail-fast; echo "gate exit=$?"
gate exit=0
$ grep -c 'test result: ok' local.log
24
```
No test result line with a non-zero ignored count (only test names containing "ignored" matched).

## ci.yml triggers, permissions, concurrency, timeout, runner, env, steps order, pinned cache
Status: PROVEN (by file inspection; ci.yml read at 177334d)
```console
$ cat .github/workflows/ci.yml   (trimmed to decisive lines)
on: push: branches: [main]; pull_request: branches: [main]; workflow_call:
permissions: contents: read
concurrency: group: ci-${{ github.ref }}, cancel-in-progress: true
env: RUST_BACKTRACE "1", BATON_NO_UPDATE_CHECK "1"
runs-on: ubuntu-24.04 ; timeout-minutes: 30
steps: Checkout, Install toolchain (rustup show), Cache cargo (Swatinem/rust-cache@6323deb1...), cargo fmt, cargo clippy, cargo test
```
All items match; no tag trigger, no other-branch push trigger.

## actionlint reports no errors
Status: PROVEN
```console
$ docker run --rm -v "$PWD":/repo -w /repo rhysd/actionlint:latest -color=false; echo "actionlint exit=$?"
actionlint exit=0
```

## Plain push to feature branch with no PR starts no run
Status: PROVEN
```console
$ git push origin feat/ci-release-update-check
 * [new branch]      feat/ci-release-update-check -> feat/ci-release-update-check
$ sleep 20; gh run list --branch feat/ci-release-update-check --event push --json databaseId,event,status
[]
$ gh run list --branch feat/ci-release-update-check --json databaseId,event
[]
```

## PR produces a green run executing full suite, all e2e tests, `test result: ok` per binary, none ignored
Status: NOT PROVEN (run did not start; GitHub billing block, not a code failure)
```console
$ gh pr create --draft --base main --fill
https://github.com/ga-lep/baton/pull/4
$ gh run watch 37935358104 --exit-status
Run CI (37935358104) has already completed with 'failure'
exit=1
$ gh run view 37935358104
X feat/ci-release-update-check CI ga-lep/baton#4 · 37935358104
Triggered via pull_request less than a minute ago
JOBS
X gate in 3s (ID 113835917792)
ANNOTATIONS
X The job was not started because recent account payments have failed or your spending limit needs to be increased. Please check the 'Billing & plans' section in your settings
$ gh run view --log-failed 37935358104
log not found: 113835917792
```
The workflow was triggered by the PR (so the trigger works), but no step ran; no log exists. Locally the same commands give 24 `test result: ok`. Needs the account billing/spending limit fixed, then re-run (`gh run rerun 37935358104`).

## If any e2e fails on runner but not locally, fix goes in this task
Status: NOT PROVEN (no runner execution happened)

## Re-run after repo made public (run 37935358104, head sha 177334db71f1f8740705abd919f19edb1a67fddd)
Status: PROVEN (supersedes the two NOT PROVEN sections above)
```console
$ gh run watch 37935358104 --repo ga-lep/baton --exit-status >/dev/null; echo "exit=$?"
exit=0
$ gh run view 37935358104 --repo ga-lep/baton
✓ feat/ci-release-update-check CI ga-lep/baton#4 · 37935358104
Triggered via pull_request about 2 minutes ago
JOBS
✓ gate in 2m14s (ID 113837739985)
$ gh run view 37935358104 --repo ga-lep/baton --log > log.txt
$ grep -c 'test result: ok' log.txt
24
$ grep -c 'test result:' log.txt
24
$ grep 'test result:' log.txt | grep -v ' 0 ignored'
(no output: every binary reports 0 ignored)
$ grep -E 'Running |Doc-tests|test result:' log.txt   # trimmed to e2e binaries, log prefixes stripped
Running tests/e2e_daemon.rs      -> ok. 3 passed; 0 failed; 0 ignored
Running tests/e2e_doctor.rs      -> ok. 4 passed; 0 failed; 0 ignored
Running tests/e2e_hook.rs        -> ok. 8 passed; 0 failed; 0 ignored
Running tests/e2e_keys.rs        -> ok. 3 passed; 0 failed; 0 ignored
Running tests/e2e_notify.rs      -> ok. 3 passed; 0 failed; 0 ignored
Running tests/e2e_resume.rs      -> ok. 10 passed; 0 failed; 0 ignored
Running tests/e2e_sessions.rs    -> ok. 11 passed; 0 failed; 0 ignored
Running tests/e2e_spike.rs       -> ok. 2 passed; 0 failed; 0 ignored
Running tests/e2e_status.rs      -> ok. 3 passed; 0 failed; 0 ignored
Running tests/e2e_tui_attach.rs  -> ok. 3 passed; 0 failed; 0 ignored
Running tests/e2e_tui_projects.rs-> ok. 1 passed; 0 failed; 0 ignored
Running tests/e2e_usage.rs       -> ok. 3 passed; 0 failed; 0 ignored
```
Other binaries also ok: baton unittests 167, config_check 3, baton_core 96 + config_examples 12, baton_proto 7, baton_testkit 1, drive_smoke 1, doc-tests 5/0/0. 12 `e2e_*` binaries, 54 e2e tests, all run, none ignored. The count (24) matches the local gate. The run executed on the PR at the task commit, green.

Second criterion (runner-only e2e failure requires fix + findings update): no e2e test failed on the runner, so no fix or findings update was needed. No `#[ignore]` was used (0 ignored everywhere).

## Verdict
EVIDENCE: PROVEN
