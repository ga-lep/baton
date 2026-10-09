# Evidence — Task 7: Release workflow, release profile, 0.2.0, RELEASING.md
Commit: eb0d80e
Environment: branch pushed to origin (approved), PR #4 runs. Release run 37944050806, CI run 37944049553. Local: cargo release build; actionlint via docker rhysd/actionlint:latest.

## actionlint reports no errors
Status: PROVEN
```console
$ docker run --rm -v "$PWD":/repo -w /repo rhysd/actionlint:latest; echo "actionlint exit=$?"
actionlint exit=0
```

## Version 0.2.0, Cargo.lock refreshed, `baton --version` prints `baton 0.2.0`
Status: PROVEN (locally)
```console
$ cargo metadata --no-deps --format-version 1 --locked >/dev/null; echo locked=$?
locked=0
$ grep -n -A2 'name = "baton"$' Cargo.lock
265:name = "baton"
266-version = "0.2.0"
$ cargo build --release --locked -p baton && target/release/baton --version
    Finished `release` profile [optimized] target(s) in 28.80s
baton 0.2.0
```

## Triggers, permissions, job order, matrix, verify/build/publish logic, no third-party release action, [profile.release]
Status: PROVEN by reading .github/workflows/release.yml and Cargo.toml (static) plus actionlint; the dynamic parts were NOT exercised, see below.
- triggers: push tags `v[0-9]+.[0-9]+.[0-9]+*`; pull_request branches [main] paths [release.yml]; workflow_dispatch.
- top-level `permissions: contents: read`; only `publish` has `contents: write` and `if: startsWith(github.ref, 'refs/tags/v')`.
- gate (uses ./.github/workflows/ci.yml) -> verify -> build -> publish (needs chain).
- matrix: x86_64-unknown-linux-musl, x86_64-unknown-linux-gnu only.
- publish uses `gh release create "$GITHUB_REF_NAME" dist/* --verify-tag --generate-notes`; no release action.
- Cargo.toml `[profile.release]`: strip = "symbols", lto = "thin", codegen-units = 1.
In the release run, `publish` was skipped by the condition (shown below).

## Build-only run on PR: release run green, publish skipped, verify prints version without comparing
Status: FAILED
```console
$ gh run view 37944050806 --repo ga-lep/baton
X feat/ci-release-update-check Release ga-lep/baton#4 · 37944050806
Triggered via pull_request about 1 minute ago

JOBS
X gate / gate in 1m9s (ID 113865567475)
  ✓ cargo fmt
  ✓ cargo clippy
  X cargo test
- build (${{ matrix.target }}) (ID 113866099758)
- verify (ID 113866100154)
- publish in 0s (ID 113866100422)
ANNOTATIONS
X Process completed with exit code 101.
```
`gh run watch ... --exit-status` exit=1. The gate failed, so verify and build were skipped, and publish was skipped (as designed for non-tags). The verify log, build, archives, `.sha256` files and artifacts could therefore NOT be produced or checked in CI.

Cause (CI log of the standalone CI run 37944049553, same failure inside the gate):
```console
$ gh run view 37944049553 --repo ga-lep/baton --log-failed | grep -A3 'check_reports_up_to_date stdout'
thread 'check_reports_up_to_date' panicked at crates/baton/tests/e2e_version.rs:97:5:
assertion `left == right` failed
  left: "baton 0.2.0 is up to date (automatic checks are disabled)\n"
 right: "baton 0.2.0 is up to date\n"
test result: FAILED. 13 passed; 5 failed; ... (e2e_version)
failures: check_reports_available_update_and_writes_cache, check_reports_up_to_date,
 foreign_github_url_is_dropped, http_errors_exit_one_with_reason_and_failed_cache, newer_without_url_has_no_dangling_separator
```
ci.yml (line 19) sets `BATON_NO_UPDATE_CHECK: "1"` at workflow env, and `version --check` appends "(automatic checks are disabled)" when it is set. The e2e_version tests do not clear that variable. Reproduced locally:
```console
$ BATON_NO_UPDATE_CHECK=1 cargo test --locked -p baton --test e2e_version | grep 'test result'
test result: FAILED. 13 passed; 5 failed; ...
$ cargo test --locked -p baton --test e2e_version | grep 'test result'
test result: ok. 18 passed; 0 failed; ...
```
Fix direction (not applied): have the e2e_version tests `env_remove("BATON_NO_UPDATE_CHECK")` (or the helper that spawns baton), or drop the variable from ci.yml. Note the previous CI run (177334d, run 37935358104) was green, so the failure is from the update-check tests added since / the env var in ci.yml.

## CI workflow run on the same push is green
Status: FAILED
CI run 37944049553: conclusion failure (`cargo test` step, exit 101), same 5 tests as above.

## verify fails on tag mismatch; build smoke tests / musl static / archive layout / sha256 / artifacts; publish with SHA256SUMS
Status: NOT PROVEN (not reached in CI because the gate failed; no tag may be pushed). `tar tzf`, `sha256sum -c`, `file` on the musl binary and the gnu binary run were not possible without the artifacts.

## docs/RELEASING.md documents steps, verification, archive choice, install, opt-out, public-repo note, license note
Status: PROVEN (read): sections "Cutting a release", "Artifacts" (archive choice, musl recommended), "Verifying a download", "Installing", "Update check" (BATON_NO_UPDATE_CHECK=1 / update_check=false; repo must be public), "License" (MIT).

## Pending, user decision (not failures)
- After merge and tag v0.2.0: real release, `SHA256SUMS`, installed binary.
- Once the repo is public: `baton version --check` says up to date.
