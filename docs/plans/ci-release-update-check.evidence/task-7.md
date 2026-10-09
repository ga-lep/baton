# Evidence — Task 7: Release workflow, release profile, 0.2.0, RELEASING.md

Overall status: FAILED (run 2: musl static check in release.yml fails; see "Run 2" at the end). Run 1 below is kept as history.

# Run 1 (commit eb0d80e) — FAILED (CI gate red)
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


---

# Run 2 (commit f767110) — FAILED (1 criterion)
Commit: f767110
Environment: pushed `feat/ci-release-update-check` to origin (approved; first push attempt hit a transient error, second succeeded). CI run 37945715746 and Release run 37945716125 on PR #4. glibc local: Ubuntu GLIBC 2.39. No tag, no merge, no ready-mark, no workflow_dispatch.

## actionlint reports no errors
Status: PROVEN
```console
$ docker run --rm -v "$PWD":/repo -w /repo rhysd/actionlint:latest; echo "actionlint exit=$?"
actionlint exit=0
```

## CI workflow run is green (previous failure fixed)
Status: PROVEN
```console
$ gh run watch 37945715746 --repo ga-lep/baton --exit-status; echo $?
0
$ gh run view 37945715746 --repo ga-lep/baton --log | grep 'test result' | sed -n '1p;15p;16p'
gate	cargo test	... test result: ok. 171 passed; 0 failed; ...
gate	cargo test	... test result: ok. 18 passed; 0 failed; ... (e2e_version)
gate	cargo test	... test result: ok. 118 passed; 0 failed; ...
```
All `test result` lines are `ok` with 0 failed (e2e_version: 18 passed).

## Release run on PR: gate -> verify -> build -> publish order, gate green, verify prints version without comparing, publish skipped
Status: PROVEN for gate/verify/publish; build see below
```console
$ gh run view 37945716125 --repo ga-lep/baton   (trimmed)
✓ gate / gate in 1m37s
✓ verify in 57s
X build (x86_64-unknown-linux-musl) in 2m1s
✓ build (x86_64-unknown-linux-gnu) in 1m51s
- publish in 0s
$ grep -E 'Workspace version|Build-only' rel.log
verify  Check tag against workspace version  Workspace version: 0.2.0
verify  Check tag against workspace version  Build-only mode: skipping tag comparison.
```
Gate (same test counts as CI run: 171, 18, 118 ... all ok) passed, verify printed 0.2.0 and skipped the comparison, publish was skipped.

## build: cargo build --locked per target, smoke tests, matrix exactly musl+gnu
Status: PROVEN for gnu; FAILED for musl
```console
build (x86_64-unknown-linux-gnu)	Smoke test	baton 0.2.0
build (x86_64-unknown-linux-gnu)	Smoke test	baton 0.2.0 (protocol 6)
build (x86_64-unknown-linux-musl)	Smoke test	baton 0.2.0
build (x86_64-unknown-linux-musl)	Smoke test	baton 0.2.0 (protocol 6)
build (x86_64-unknown-linux-musl)	Smoke test	target/x86_64-unknown-linux-musl/release/baton: ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), static-pie linked, BuildID[sha1]=d08f4cca24d574169222285cb188a6d771bb2b3e, stripped
build (x86_64-unknown-linux-musl)	Smoke test	##[error]binary is not statically linked
```
Both legs: `--version` exactly `baton 0.2.0` and `BATON_NO_UPDATE_CHECK=1 baton version` OK. The musl leg FAILED the "file reports `statically linked`" assertion: `file` on this runner prints `static-pie linked` for the (genuinely static) musl PIE binary, and the workflow's `grep -q 'statically linked'` does not match that wording. So Package and Upload were skipped for musl. This is a workflow bug (assertion too narrow), not a non-static binary. Not fixed here; suggested fix direction: match `statically linked|static-pie linked`.

## Archive layout, .sha256, artifacts
Status: PROVEN for gnu only; musl NOT PROVEN (no musl artifact was produced because of the failure above)
```console
$ gh run download 37945716125 --repo ga-lep/baton -D $D/art ; find $D/art -type f
.../art/baton-x86_64-unknown-linux-gnu/baton-fd7f5f3ce6b7-x86_64-unknown-linux-gnu.tar.gz
.../art/baton-x86_64-unknown-linux-gnu/baton-fd7f5f3ce6b7-x86_64-unknown-linux-gnu.tar.gz.sha256
$ sha256sum -c *.sha256
baton-fd7f5f3ce6b7-x86_64-unknown-linux-gnu.tar.gz: OK
$ tar tzf *.tar.gz
baton-fd7f5f3ce6b7-x86_64-unknown-linux-gnu/
baton-fd7f5f3ce6b7-x86_64-unknown-linux-gnu/baton
baton-fd7f5f3ce6b7-x86_64-unknown-linux-gnu/LICENSE
baton-fd7f5f3ce6b7-x86_64-unknown-linux-gnu/README.md
$ ./baton-*/baton --version
baton 0.2.0
$ ldd --version | head -1
ldd (Ubuntu GLIBC 2.39-0ubuntu8.9) 2.39
```
Name is `baton-<12-char sha>-<target>` for non-tag runs; one top-level dir with the three files. Musl archive extraction, `sha256sum -c` for it, and local `file` on the musl binary could not be done (no artifact).

## Version, Cargo.lock, triggers, permissions, profile, publish script, RELEASING.md
Status: PROVEN statically (unchanged from Run 1 for version/lock/profile; release.yml re-read at f767110: triggers, `permissions: contents: read`, only `publish` has `contents: write` and runs on `github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')`, no third-party release action, `gh release create ... --verify-tag`). The publish job itself was skipped, so its SHA256SUMS step and release creation were not exercised. `docs/RELEASING.md` content was not re-reviewed in this run.

## verify failing on a tag mismatch; publish creating a release; "after merge and tag v0.2.0"; `version --check` up to date once a release exists
Status: PENDING (user decision; requires pushing a tag, which was forbidden). Not failures.

## Run 2 verdict
FAILED (1 criterion): musl static-link assertion in build rejects `static-pie linked`.

---

# Run 3 — commit 714888c (fix for the static-link assertion)
Commit: 714888c99558d1eccd4a16e1f8a57d8306a9fb3f, pushed to origin/feat/ci-release-update-check (user-approved). No tag, no merge, PR #4 still draft, no `gh release create`.
Runs (pull_request on PR #4): CI 37947724254, Release 37947724799. Note: GitHub checks out the PR merge ref, so artifact names carry the merge sha `27aa08146698`, not `714888c`.

## CI and release gate green
Status: PROVEN
```console
$ gh run watch 37947724254 --repo ga-lep/baton --exit-status ; echo ci=$?
ci=0
$ gh run watch 37947724799 --repo ga-lep/baton --exit-status ; echo rel=$?
rel=0
$ gh run view 37947724799 --repo ga-lep/baton
✓ gate / gate in 1m2s
✓ verify in 14s
✓ build (x86_64-unknown-linux-musl) in 1m32s
✓ build (x86_64-unknown-linux-gnu) in 1m14s
- publish in 0s
ARTIFACTS
baton-x86_64-unknown-linux-musl
baton-x86_64-unknown-linux-gnu
```
Both runs exit 0; gate green; publish skipped (`-`).

## verify prints the version without comparing; publish skipped
Status: PROVEN
```console
$ gh run view --job 113878717631 --repo ga-lep/baton --log   (Check tag against workspace version, trimmed)
  EVENT: pull_request
Workspace version: 0.2.0
Build-only mode: skipping tag comparison.
```

## musl build leg: smoke tests, package, upload
Status: PROVEN
```console
$ gh run view --job 113878838877 --repo ga-lep/baton --log   (Smoke test / Package / Upload, trimmed)
Smoke test  baton 0.2.0
Smoke test  baton 0.2.0 (protocol 6)
Smoke test  target/x86_64-unknown-linux-musl/release/baton: ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), static-pie linked, BuildID[sha1]=d08f4cca..., stripped
Package     -rw-r--r-- 1 runner runner 3255335 Oct  9 14:57 baton-27aa08146698-x86_64-unknown-linux-musl.tar.gz
Package     -rw-r--r-- 1 runner runner     118 Oct  9 14:57 baton-27aa08146698-x86_64-unknown-linux-musl.tar.gz.sha256
Upload      name: baton-x86_64-unknown-linux-musl
Upload      With the provided path, there will be 2 files uploaded
Upload      Artifact baton-x86_64-unknown-linux-musl successfully finalized. Artifact ID 11624817575
```
The step script (`set -euo pipefail`) compares `--version` to `baton $VERSION` (VERSION=0.2.0), runs `BATON_NO_UPDATE_CHECK=1 baton version`, then the widened `file` grep (`statically linked|static-pie linked`) and the readelf INTERP/NEEDED checks; all passed, so Package and Upload ran. The earlier failure (`static-pie linked` rejected) is resolved.

## gnu leg still passes
Status: PROVEN
```console
$ gh run view --job 113878839281 --repo ga-lep/baton --log   (trimmed)
Smoke test  baton 0.2.0
Smoke test  baton 0.2.0 (protocol 6)
Upload      name: baton-x86_64-unknown-linux-gnu
```

## Downloaded artifacts
Status: PROVEN
```console
$ gh run download 37947724799 --repo ga-lep/baton -D $D/art ; find $D/art -type f
art/baton-x86_64-unknown-linux-musl/baton-27aa08146698-x86_64-unknown-linux-musl.tar.gz
art/baton-x86_64-unknown-linux-musl/baton-27aa08146698-x86_64-unknown-linux-musl.tar.gz.sha256
art/baton-x86_64-unknown-linux-gnu/baton-27aa08146698-x86_64-unknown-linux-gnu.tar.gz.sha256
art/baton-x86_64-unknown-linux-gnu/baton-27aa08146698-x86_64-unknown-linux-gnu.tar.gz
$ sha256sum -c *.sha256 ; tar tzf *.tar.gz     (per target dir)
baton-27aa08146698-x86_64-unknown-linux-musl.tar.gz: OK
baton-27aa08146698-x86_64-unknown-linux-musl/
baton-27aa08146698-x86_64-unknown-linux-musl/baton
baton-27aa08146698-x86_64-unknown-linux-musl/LICENSE
baton-27aa08146698-x86_64-unknown-linux-musl/README.md
baton-27aa08146698-x86_64-unknown-linux-gnu.tar.gz: OK
baton-27aa08146698-x86_64-unknown-linux-gnu/
baton-27aa08146698-x86_64-unknown-linux-gnu/baton
baton-27aa08146698-x86_64-unknown-linux-gnu/LICENSE
baton-27aa08146698-x86_64-unknown-linux-gnu/README.md
$ ./baton-*-musl/baton --version
baton 0.2.0
$ file ./baton-*-musl/baton
ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), static-pie linked, BuildID[sha1]=d08f4cca24d574169222285cb188a6d771bb2b3e, stripped
$ readelf -lW baton | grep -c INTERP
0
$ readelf -d baton | grep -c NEEDED
0
```
Both `.sha256` OK, one top-level dir with the three files each, musl binary prints exactly `baton 0.2.0`, static-pie, no INTERP, no NEEDED. The extracted musl binary's BuildID matches the one in the CI log.

## actionlint
Status: PROVEN
```console
$ docker run --rm -v "$PWD":/repo -w /repo rhysd/actionlint:latest ; echo actionlint=$?
actionlint=0
```

## Pending (user decision, not failures)
Publish after a `v0.2.0` tag (verify tag mismatch failure, publish job, SHA256SUMS, release creation) and `baton version --check` reporting up to date once a release exists. Not exercised: tags were forbidden.

## Run 3 verdict
PROVEN for everything observable without a tag. Overall Task 7 status: PROVEN (tag/publish items pending).
