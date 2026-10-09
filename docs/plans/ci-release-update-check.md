# CI, Releases and Update Check Plan
Status: APPROVED

## Goal
Add a GitHub Actions pipeline to `github.com/ga-lep/baton`. It runs the project gate (`cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test`) on every push to `main` and on every pull request that targets `main`. When a `v*` tag is pushed, it builds the `baton` binary for Linux x86_64 (a static musl build plus a glibc build) and publishes it with checksums and the `LICENSE` as a GitHub Release. The project is relicensed from MIT to the PolyForm Noncommercial License 1.0.0, which makes it source-available, not open source. The version is bumped to `0.2.0` for the first release. The `baton` binary also learns to tell the user when a newer release exists: through an explicit `baton version --check`, a line in `baton doctor`, and a non-blocking notice in the TUI. The check uses unauthenticated GitHub API calls. It is cached, can be disabled, and degrades quietly when offline or when no public release is reachable. It is never reachable from the `baton hook`, `baton statusline` or daemon paths, which must stay silent and fast.

## Verified findings (checked 2026-10-09 on this machine)
1. **The e2e tests run headless as-is.** `setsid -w env -u DBUS_SESSION_BUS_ADDRESS -u DISPLAY -u WAYLAND_DISPLAY -u XDG_RUNTIME_DIR -u TERM -u COLORTERM CI=true cargo test --locked </dev/null` passed every test in about 25 s. That means no controlling TTY, no D-Bus, no runtime dir and no `TERM`. The tests stay headless because:
   - Every e2e `Env` sets `BATON_CONFIG`, `BATON_STATE_DIR`, `BATON_RUNTIME_DIR` and `BATON_NOTIFY_SINK=off|log`.
   - PTYs come from `portable-pty` (`/dev/ptmx`, present on GitHub's ubuntu VMs).
   - `baton-drive` sets `TERM=xterm-256color` for its child.
   - `doctor`'s notify check only WARNs without D-Bus.

   The remaining CI risks are timing under a loaded 4-vCPU runner (tests use 15 s waits, which is generous) and the external tools `bash`, `sh` and `tput`, all present on `ubuntu-24.04` images.
2. **The binary has no native library dependencies besides libc.** `ldd target/debug/baton` lists only `libc`, `libgcc_s` and `ld-linux`. `notify-rust` uses pure-Rust `zbus`, so a static `x86_64-unknown-linux-musl` build is feasible. `ring` (pulled in by the HTTP client, see Task 3) needs a C compiler, so the musl build needs `musl-tools` (`musl-gcc`).
3. **The repo is currently PRIVATE** (`gh repo view` → `"visibility":"PRIVATE"`).
   - **What fails until then:** unauthenticated `GET https://api.github.com/repos/ga-lep/baton/releases/latest` returns 404, and release assets cannot be downloaded without auth.
   - **The plan assumes the repo will be made public** before the update check and public downloads are relied on. No token handling is built.
   - **Until then:** the check fails quietly and is treated like offline. There is no TUI notice, doctor shows a WARN, `baton version --check` exits 1 with a one-line reason, and the next retry waits 6 h. `docs/RELEASING.md` states this.
4. There are no tags or releases yet. The workspace version is `0.1.0` (`[workspace.package]`), and `baton --version` already prints `CARGO_PKG_VERSION` through clap's `version` attribute. `PROTOCOL_VERSION` lives in `crates/baton-proto/src/msg.rs`.
5. The local toolchain is `rustc 1.97.1` and the only installed target is `x86_64-unknown-linux-gnu`. Neither `actionlint` nor `act` is installed, but `docker` is, so workflows are linted with `docker run rhysd/actionlint`.
6. Crate versions on crates.io: `ureq 3.4.2` and `semver 1.0.28`. `ureq` features: `default = [rustls, gzip]`, and `rustls = [rustls-no-provider, _ring, rustls-webpki-roots]`.
7. `Config` parsing uses `#[serde(deny_unknown_fields)]` on `RawConfig`, so a new top-level key has to be added to `RawConfig`, `Config` and `Default`.
8. The TUI event loop (`crates/baton/src/tui/event_loop.rs`) is a `tokio::select!` over terminal events, the daemon connection and a timer. A new branch fits there to receive a background result.
9. **License.**
   - All four crates use `license.workspace = true`, so changing `[workspace.package] license` relicenses every crate in one place.
   - The repo has no `LICENSE` and no `README` file today.
   - `PolyForm-Noncommercial-1.0.0` is an official SPDX identifier, and SPDX marks it `isOsiApproved: false`, so the project becomes source-available, not open source.
   - The canonical Markdown text is at `https://raw.githubusercontent.com/polyformproject/polyform-licenses/1.0.0/PolyForm-Noncommercial-1.0.0.md`: 73 lines, sha256 `c0ea4a896d2c8c394b29f9427589996db826cd501c512279ff0ed3ef48fabbe5`.
   - The license requires that recipients get the terms plus any lines starting with `Required Notice:`. Its example is `Required Notice: Copyright Yoyodyne, Inc. (http://example.com)`.

## Decisions

### CI
- **Workflow:** `.github/workflows/ci.yml`.
- **Triggers:**
  - `push` to `main` only (`branches: [main]`; tags are excluded, because the release workflow runs the gate itself);
  - `pull_request` with `branches: [main]` (PRs targeting `main`);
  - `workflow_call`, so `release.yml` can reuse the gate.

  Feature-branch pushes without a PR do not run CI.
- **Concurrency:** `ci-${{ github.ref }}` with `cancel-in-progress: true`. A new push to a PR cancels the previous run.
- **Runner:** one job on `ubuntu-24.04`, `timeout-minutes: 30`, `permissions: contents: read`.
- **Steps:**
  1. Checkout.
  2. Install the toolchain from `rust-toolchain.toml` (`rustup show`).
  3. `Swatinem/rust-cache`.
  4. Run the gate as three named steps: `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, and `cargo test --locked --no-fail-fast`.
- **Job environment:** `RUST_BACKTRACE=1` and `BATON_NO_UPDATE_CHECK=1` (belt-and-braces: tests never touch the network).
- **Toolchain pin (confirmed):** a new `rust-toolchain.toml` with `channel = "1.97.1"` and `components = ["rustfmt", "clippy"]`. A new stable release can then not turn CI red through a new clippy lint. Bumps are deliberate commits. This matches the local toolchain and also applies to local builds.
- **Action pinning:** third-party actions (`Swatinem/rust-cache`) are pinned by full commit SHA with a version comment. `actions/checkout` is pinned by major version or SHA, and the same rule applies everywhere.

### Releases
- **Trigger:** pushing a tag that matches `v[0-9]+.[0-9]+.[0-9]+*` (for example `v0.2.0`, or `v0.2.0-rc.1` for a pre-release). The workflow also runs in **build-only mode** on pull requests that touch `.github/workflows/release.yml`, and on `workflow_dispatch`. In that mode it builds and uploads workflow artifacts but never publishes. This lets the pipeline be validated in its own PR before the first real tag.
- **Version source of truth:** `[workspace.package] version` in `Cargo.toml`. A `verify` job fails the release unless the tag minus its leading `v` equals that version, read through `cargo metadata --no-deps --format-version 1`. The version is bumped to **`0.2.0`** in Task 7, so the first release is `v0.2.0`. The release procedure is:
  1. Bump the version in `Cargo.toml`.
  2. Run `cargo check` to refresh `Cargo.lock`.
  3. Merge to `main` through a PR.
  4. `git tag -a vX.Y.Z -m vX.Y.Z <main-sha> && git push origin vX.Y.Z`.

  This is documented in `docs/RELEASING.md`.
- **Pipeline:**
  1. `gate`: `uses: ./.github/workflows/ci.yml`.
  2. `verify`: tag matches version.
  3. `build`: a matrix over targets.
  4. `publish`: only on tag refs, `permissions: contents: write`.

  `publish` uses the preinstalled `gh` CLI with `GITHUB_TOKEN`, so no third-party release action is involved: `gh release create "$TAG" dist/* --verify-tag --title "$TAG" --generate-notes`, plus `--prerelease` when the tag contains `-`.
- **Targets: Linux x86_64 only** (all built on `ubuntu-24.04` with `cargo build --release --locked -p baton --target <T>`):
  - `x86_64-unknown-linux-musl`: static and **the recommended download**. It runs on any x86_64 Linux. Needs `apt-get install musl-tools`.
  - `x86_64-unknown-linux-gnu`: **kept** because it costs only one matrix entry. It shares every step with the musl entry; only the `musl-tools` install is skipped through `if: matrix.target == …`. Once the repo is public, hosted-runner minutes are free. It links dynamically against glibc ≥ 2.39 (the runner's glibc), and the release notes and `RELEASING.md` state that floor.

  No aarch64 target is built.
- **Release profile:** add `[profile.release] strip = "symbols"`, `lto = "thin"` and `codegen-units = 1` to the workspace `Cargo.toml`. This keeps the binary small. Debug and test builds are unaffected.
- **Artifact naming:**
  - Each target gets `baton-<tag>-<target>.tar.gz`, for example `baton-v0.2.0-x86_64-unknown-linux-musl.tar.gz`. It holds one top-level dir `baton-<tag>-<target>/` containing `baton`, `LICENSE` and `README.md`.
  - Each archive has a matching `baton-<tag>-<target>.tar.gz.sha256` in `sha256sum` format.
  - One `SHA256SUMS` file covers all archives.

  Users can verify with `sha256sum -c SHA256SUMS --ignore-missing`.
- **Smoke test in `build`:** before packaging, each built binary must:
  - pass `./baton --version` (prints `baton X.Y.Z`, equal to the tag);
  - pass `BATON_NO_UPDATE_CHECK=1 ./baton version` (Task 3);
  - for musl, show `file baton` reporting `statically linked`.

### License
- **License:** PolyForm Noncommercial License 1.0.0, copyright holder `ga-lep`. It forbids commercial use and makes the project **source-available, not open source** (not OSI-approved).
- **`LICENSE`:** the canonical Markdown text, unmodified (sha256 as in finding 9), followed by a blank line and the line `Required Notice: Copyright ga-lep (https://github.com/ga-lep/baton)`.
- **Cargo:** `[workspace.package] license = "PolyForm-Noncommercial-1.0.0"`, the official SPDX id. It is used instead of `license-file` or a `LicenseRef-`, because SPDX lists it and every crate already inherits the field.
- **README:** a new minimal `README.md` (what Baton is in two lines, install pointer to `docs/RELEASING.md`, and a License section stating the non-commercial and source-available terms). It is shipped in the archives next to `LICENSE`.

### Update check
- **Surfaces:**
  1. **`baton version [--check]`** (new subcommand).
     - Without `--check`, it prints `baton 0.2.0 (protocol 1)` and never touches the network.
     - With `--check`, it always queries GitHub (ignoring the cache freshness and the disable switches, because the user asked explicitly), updates the cache and prints one of:
       - `baton 0.2.0 is up to date`
       - `baton 0.3.0 is available (you have 0.2.0): <html_url>`
       - `could not check for updates: <reason>`
     - Exit codes: 0 when up to date, 0 when an update is available, 1 when the check failed. The output is stable text for scripts.
  2. **`baton doctor`**: a new `version:` line, which is never a FAIL. It reads:
     - `PASS version: 0.2.0 (latest)`
     - `WARN version: 0.3.0 is available (you have 0.2.0): <url>`
     - `WARN version: could not check for updates (<reason>)`
     - `PASS version: 0.2.0 (update check disabled)`

     It uses the cache when fresh, otherwise it makes a network call with a 3 s timeout.
  3. **TUI**: at startup it reads the cache synchronously (a tiny file). If the cache is stale and checks are enabled, it fetches on a detached `std::thread` and hands the result to the event loop over a `tokio::sync::oneshot`. A newer version is shown as a right-aligned bottom title on the Projects block: ` v0.3.0 available `. The TUI never waits for the thread: quitting just exits the process. Errors (offline, 404, rate limit) go to `tracing::debug!` only and show nothing on screen, because the TUI owns the terminal.
- **Never reachable from:** `baton hook`, `baton statusline` (both dispatched in `main.rs` before clap), the daemon (`baton daemon start --foreground`), `baton config check`, `baton spike` and `baton debug`. Tests prove that no connection is made (Task 5).
- **Query:** unauthenticated `GET https://api.github.com/repos/ga-lep/baton/releases/latest`.
  - The full URL can be overridden with the env var `BATON_UPDATE_URL`, used by tests to point at a local fake server.
  - Headers: `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`, `User-Agent: baton/<version>`. No `Authorization` header is ever sent.
  - `If-None-Match: <etag>` is sent when a cached ETag exists, so a `304` refreshes `checked_at` without a body.

  `/releases/latest` already excludes drafts and pre-releases. Only `tag_name` and `html_url` are read from the response. The anonymous rate limit (60 requests/h/IP) is far above what the 24 h cache needs.
- **HTTP client (confirmed):** `ureq 3` with `default-features = false, features = ["rustls"]`, which gives rustls with ring and webpki-roots (no OpenSSL, no gzip). Further settings:
  - It is blocking and needs no tokio integration; the TUI runs it on its own thread.
  - Its global timeout is 3 s for `version --check` and doctor, and 2 s for the TUI.
  - `http_status_as_error(false)`, so statuses map to readable reasons:
    - 404 reads `no public release found`; this is what a private repo or a repo without releases returns.
    - 403 or 429 reads `rate limited`.
    - Other statuses read `HTTP <code>`.
  - The response body is capped at 1 MiB.
  - It honours `HTTPS_PROXY` through ureq's env proxy support. Verify the ureq 3 default during Task 3 and set it explicitly if needed.

  Cost: about 1–2 MB of stripped binary and a `ring` C build. Task 3 records the measured size delta in the commit message. The rejected alternatives are:
  - `reqwest`: heavier, pulls hyper and tokio features we do not need for one GET.
  - Shelling out to `curl`: a runtime dependency with variable flags and proxy/CA behaviour, and harder to test.
- **Version comparison:** `semver` 1 in `baton-core`. The tag has its leading `v` stripped. An update is shown only when `latest > current` under semver ordering, so `0.3.0-rc.1 < 0.3.0`, and a pre-release build ahead of the latest stable sees no notice. An unparseable tag gives "unknown", no notice, and a doctor WARN.
- **Cache:** `<state_dir>/update-check.json` (state dir from `paths::state_dir()`, so tests isolate it through `BATON_STATE_DIR`). Contents: `{ "checked_at": <unix secs>, "latest": "v0.3.0" | null, "html_url": "…" | null, "etag": "…" | null, "ok": true|false }`.
  - **Fresh:** less than 24 h after a successful check, or less than 6 h after a failed one. The failure TTL covers offline machines and the private-repo 404: they retry at most every 6 h.
  - **Stale:** a `checked_at` in the future (clock skew) counts as stale.
  - **Writes and reads:** writes are atomic (temp file plus rename in the same dir). A missing or corrupt file counts as "never checked". Failure to write the cache is ignored.
- **Disabling:** `BATON_NO_UPDATE_CHECK=1` (any non-empty value other than `0`) or a top-level config key `update_check = false` (default `true`). Both turn off the automatic checks (TUI and doctor). The explicit `baton version --check` still runs, but prints `(automatic checks are disabled)` after its result.
- **No self-update:** this plan only notifies. Installing is manual, following `docs/RELEASING.md`.

## Conventions (apply to every task)
- The gate stays green after every task:
  ```
  cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
  ```
- Workspace lints still apply: no `unwrap`/`expect` outside tests.
- Tests never reach the real GitHub API. Network behaviour is tested against a fake HTTP server on `127.0.0.1:0`, added to `baton-testkit` in Task 3, through `BATON_UPDATE_URL`. Every existing e2e `Env` that starts the TUI or doctor sets `BATON_NO_UPDATE_CHECK=1`, unless the test is about the update check.
- `Language:` is `rust` for every task, because the language registry is limited to go/typescript/rust. Tasks 1, 6 and 7 are mostly YAML or docs. Their Evidence lines say how they are validated: `actionlint` through docker, plus a PR-triggered run inspected with `gh run`.
- `D` is a scratch dir with `export BATON_CONFIG=$D/config.toml BATON_STATE_DIR=$D/state BATON_RUNTIME_DIR=$D/run`, as in `docs/plans/baton-mvp.md`. `actionlint` means `docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:latest -color`.

### Task 1: CI workflow running the gate on pushes to main and on PRs
Language: rust
Files: `.github/workflows/ci.yml`, `rust-toolchain.toml`
Depends on: none
Acceptance criteria:
- `rust-toolchain.toml` pins `1.97.1` with `rustfmt` and `clippy`. The local gate still passes.
- `ci.yml` triggers on `push` with `branches: [main]` (no tags), `pull_request` with `branches: [main]`, and `workflow_call`. There is no trigger for pushes to other branches.
- `ci.yml` has `permissions: contents: read`, a per-ref concurrency group with `cancel-in-progress: true` and `timeout-minutes: 30`.
- The job runs on `ubuntu-24.04` with `BATON_NO_UPDATE_CHECK=1` and `RUST_BACKTRACE=1`. It runs `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` and `cargo test --locked --no-fail-fast` as separate named steps, after `Swatinem/rust-cache` (pinned by SHA).
- `actionlint` reports no errors.
- A pull request from `feat/ci-release-update-check` to `main` produces a green run that executes the full test suite, including all `e2e_*` tests: the log shows `test result: ok` for every test binary and no `ignored` e2e tests.
- A plain push to the feature branch with no PR open starts no run.
- If any e2e test fails on the runner but not locally, the fix goes into this task and the plan's findings are updated. A blanket `#[ignore]` is not acceptable.
Evidence: `actionlint` exits 0. Then `git push origin feat/ci-release-update-check && gh pr create --draft --base main --fill`, followed by `gh run watch $(gh run list --workflow ci.yml --branch feat/ci-release-update-check --event pull_request --limit 1 --json databaseId -q '.[0].databaseId') --exit-status`, exits 0. `gh run view --log <id> | grep -c 'test result: ok'` matches the local count.

### Task 2: Update-check core logic and `update_check` config key (baton-core)
Language: rust
Files: `crates/baton-core/src/update.rs` (new), `crates/baton-core/src/lib.rs`, `crates/baton-core/src/config.rs`, `crates/baton-core/src/paths.rs`, `crates/baton-core/Cargo.toml` (`semver.workspace = true`), `Cargo.toml` (workspace dep `semver = "1"`)
Depends on: none
Acceptance criteria:
- `update::Outcome` is computed by `compare(current: &str, latest_tag: &str)`. It returns `UpToDate`, `Newer { latest }` or `Unknown(reason)`, and strips a leading `v`. Table tests cover:
  - `0.1.0` vs `v0.2.0` → `Newer`;
  - `0.2.0` vs `v0.2.0` → `UpToDate`;
  - `0.3.0-rc.1` vs `v0.2.0` → `UpToDate`;
  - `0.2.0` vs `v0.3.0-rc.1` → `Newer`;
  - `0.2.0` vs `nightly` → `Unknown`.
- `update::parse_release(body: &str)` extracts `tag_name` and `html_url` and ignores other fields. Missing `tag_name` is an error. It is unit-tested with a trimmed real GitHub response.
- `update::Cache` round-trips through JSON. `Cache::load(path)` returns `None` for a missing, empty or corrupt file. `Cache::store(path)` writes atomically (temp file plus rename) and creates the parent dir.
- `Cache::is_fresh(now)` follows the rules, each with a unit test: 24 h after success, 6 h after failure, and a future `checked_at` is stale.
- `update::enabled(config_flag, getenv)` is false when `BATON_NO_UPDATE_CHECK` is set to a non-empty value other than `0`, or when `config_flag` is false. Unit-tested with an injected getenv, in the existing `Getenv` style.
- `paths::update_cache_path()` returns `<state_dir>/update-check.json`.
- The config gets a top-level `update_check = <bool>` (default `true`). An unknown key is still rejected, and `update_check = "yes"` is a config error naming the key.
- No network code and no tokio in `baton-core`.
Evidence: none — pure library code, covered by `cargo test -p baton-core update::` and `cargo test -p baton-core config::`. It is exercised from outside in Task 3.

### Task 3: HTTP fetcher and `baton version [--check]`
Language: rust
Files: `crates/baton/src/update.rs` (new: `fetch(url, timeout, etag) -> Result<Fetched, String>` and `check(force, timeout) -> Result<Outcome, String>`, the cache-aware entry point), `crates/baton/src/cmd/version.rs` (new), `crates/baton/src/cmd/mod.rs`, `crates/baton/src/cli.rs`, `crates/baton/src/main.rs`, `crates/baton/Cargo.toml` (`ureq = { version = "3", default-features = false, features = ["rustls"] }`), `crates/baton-testkit/src/release_server.rs` (new: a tiny threaded HTTP/1.1 server on `127.0.0.1:0` with canned status, body, ETag and delay, which counts the connections it accepts and records request headers), `crates/baton-testkit/src/lib.rs`, `crates/baton/tests/e2e_version.rs` (new)
Depends on: Task 2
Acceptance criteria:
- `baton --help` lists `version`. `baton version` prints `baton <CARGO_PKG_VERSION> (protocol <PROTOCOL_VERSION>)`, exits 0 and makes no connection: the fake server counts 0 with `BATON_UPDATE_URL` pointing at it.
- With the fake server returning `{"tag_name":"v99.0.0","html_url":"http://x/r"}`, `baton version --check` prints `baton 99.0.0 is available (you have <ver>): http://x/r`, exits 0 and writes `$BATON_STATE_DIR/update-check.json` with `ok: true`.
- With `tag_name` equal to the current version, it prints `baton <ver> is up to date` and exits 0.
- Error cases:
  - Fake server status 404 → prints `could not check for updates: no public release found`, exits 1, and the cache has `ok: false`.
  - Status 403 or 429 → reason `rate limited`.
  - Connection refused (closed port) → exits 1 within 3 s.
  - A server that accepts but never answers → exits 1 in under 4 s (timeout 3 s).
- The fake server sees `User-Agent: baton/<ver>`, `Accept: application/vnd.github+json` and `X-GitHub-Api-Version: 2022-11-28`, and never an `Authorization` header, even when `GH_TOKEN` and `GITHUB_TOKEN` are set in the environment.
- With a cached ETag, the request carries `If-None-Match`. A `304` keeps the cached `latest` and updates `checked_at`.
- `BATON_NO_UPDATE_CHECK=1 baton version --check` still queries, and prints `(automatic checks are disabled)` after the result.
- Bodies larger than 1 MiB are rejected without reading them fully.
- `cargo tree -p baton -i openssl-sys` finds nothing: no OpenSSL.
- The commit message records the stripped release size of `baton` before and after adding `ureq`.
Evidence: `target/debug/baton version` prints `baton 0.1.0 (protocol 1)`. `target/debug/baton version --check; echo $?` against the real API prints `could not check for updates: no public release found` and `1` while the repo is private or has no release. After the repo is public and `v0.2.0` is published, it prints `baton 0.2.0 is up to date` and `0`.

### Task 4: `baton doctor` version line
Language: rust
Files: `crates/baton/src/cmd/doctor.rs`, `crates/baton/tests/e2e_doctor.rs`
Depends on: Task 3
Acceptance criteria:
- Doctor prints exactly one `version:` line after the notify check, with the four wordings from Decisions. It is never `FAIL`, so an offline machine or a private repo keeps doctor's exit code unchanged.
- With a fresh cache (`ok: true`, `latest` newer), doctor makes no connection (fake server count 0) and prints the WARN with the cached URL.
- With a stale cache and the fake server answering, doctor makes exactly one connection and rewrites the cache.
- With a fresh failed cache (`ok: false`, less than 6 h old), doctor makes no connection and prints `WARN version: could not check for updates (last check failed)`.
- With `BATON_NO_UPDATE_CHECK=1` or `update_check = false`, it prints `PASS version: <ver> (update check disabled)` and makes no connection.
- With an unreachable `BATON_UPDATE_URL`, doctor prints `WARN version: could not check for updates (…)` and still exits 0 on an otherwise healthy profile, finishing within its usual time plus 3 s.
- The existing `e2e_doctor` tests set `BATON_NO_UPDATE_CHECK=1` and keep their assertions.
Evidence: `BATON_UPDATE_URL=http://127.0.0.1:9/ baton doctor --no-probe; echo $?` shows `WARN version: could not check for updates (…)` and exit code `0` when the config is valid. `BATON_NO_UPDATE_CHECK=1 baton doctor --no-probe` shows `PASS version: 0.1.0 (update check disabled)`.

### Task 5: Non-blocking TUI notice, and proof that the hook, statusline and daemon paths never check
Language: rust
Files: `crates/baton/src/tui/event_loop.rs`, `crates/baton/src/tui/app.rs` (`update_available: Option<String>`), `crates/baton/src/tui/ui.rs` (bottom title on the Projects block), `crates/baton/src/update.rs` (`spawn_background(timeout) -> oneshot::Receiver<Option<String>>`), `crates/baton/tests/e2e_update_paths.rs` (new), existing `crates/baton/tests/e2e_tui_*.rs`, `e2e_keys.rs`, `e2e_notify.rs`, `e2e_usage.rs`, `e2e_sessions.rs`, `e2e_resume.rs`, `e2e_status.rs` and `e2e_hook.rs` `Env`s (add `BATON_NO_UPDATE_CHECK=1`)
Depends on: Task 3
Acceptance criteria:
- At TUI startup, the update logic behaves as follows:
  - A fresh cache with a newer `latest` shows ` v<latest> available ` on the Projects block without any connection.
  - A stale cache starts one background fetch (2 s timeout). The first frame is drawn before the fetch completes.
  - A disabled check (env or config) reads nothing and fetches nothing.
- e2e with `baton-drive` and a fake server returning `v99.0.0`: the screen eventually contains `v99.0.0 available`, and the cache file is written.
- e2e with a fake server returning 404, which is the private-repo case: no notice and no error text appear on screen, stderr is empty, and the cache has `ok: false`.
- e2e with a fake server that accepts and never answers: the TUI draws its sidebar within the usual wait, and after `q` the process exits within 1 s, so the detached thread does not delay exit. The screen shows no error text and stderr is empty.
- A unit test on `ui::draw` (existing `TestBackend` style) shows the notice when `update_available` is `Some` and nothing when it is `None`.
- `e2e_update_paths.rs` points `BATON_UPDATE_URL` at a counting fake server with an empty state dir and update checks enabled. It runs `echo '{}' | baton hook Stop`, `echo '{}' | baton statusline`, `baton daemon start`/`stop`, `baton config check` and `baton version`, then asserts:
  - the fake server accepted 0 connections;
  - `baton hook` and `baton statusline` printed nothing and exited 0;
  - no `update-check.json` was created.
- `baton hook Stop` with no daemon still returns in under 1 s, as it does today. The test asserts wall time.
Evidence: `drive --size 30x120 --step 'wait:v99.0.0 available' --step dump --step 'send:q' --step 'expect-exit:0' -- target/debug/baton`, run with `BATON_UPDATE_URL` pointing at a fake server returning `v99.0.0`, prints a screen with the notice. `echo '{}' | BATON_UPDATE_URL=http://127.0.0.1:1/ baton hook Stop; echo $?` prints only `0`.

### Task 6: Relicense to PolyForm Noncommercial 1.0.0
Language: rust
Files: `LICENSE` (new), `README.md` (new), `Cargo.toml` (`[workspace.package] license`)
Depends on: none
Acceptance criteria:
- `LICENSE` consists of the canonical PolyForm Noncommercial 1.0.0 Markdown text, byte-for-byte: `head -n 73 LICENSE | sha256sum` gives `c0ea4a896d2c8c394b29f9427589996db826cd501c512279ff0ed3ef48fabbe5`. The text is followed by one blank line and the line `Required Notice: Copyright ga-lep (https://github.com/ga-lep/baton)`.
- `[workspace.package]` has `license = "PolyForm-Noncommercial-1.0.0"`, and no crate has `license = "MIT"` any more: `grep -rn 'MIT' Cargo.toml crates/*/Cargo.toml` finds nothing. All four crates still use `license.workspace = true`.
- `cargo metadata --no-deps --format-version 1 | jq -r '.packages[].license' | sort -u` prints exactly `PolyForm-Noncommercial-1.0.0`.
- `README.md` contains:
  - a two-line description of Baton;
  - a pointer to `docs/RELEASING.md` for installing;
  - a `## License` section stating that Baton is licensed under the PolyForm Noncommercial License 1.0.0, that commercial use is not permitted, and that this makes the project **source-available, not open source** (the license is not OSI-approved), with the license URL.
- The gate still passes.
Evidence: `cargo metadata --no-deps --format-version 1 | jq -r '.packages[].license' | sort -u` prints `PolyForm-Noncommercial-1.0.0`. `tail -n 1 LICENSE` prints the Required Notice line.

### Task 7: Release workflow, release profile, version bump to 0.2.0 and release docs
Language: rust
Files: `.github/workflows/release.yml`, `Cargo.toml` (`[profile.release]`, `[workspace.package] version = "0.2.0"`), `Cargo.lock`, `docs/RELEASING.md`
Depends on: Task 1, Task 3 (the smoke test calls `baton version`), Task 6 (the archives ship `LICENSE` and `README.md`)
Acceptance criteria:
- `[workspace.package] version` is `0.2.0`, and `Cargo.lock` is refreshed so the `--locked` builds pass. `baton --version` prints `baton 0.2.0`. Tests compare against `CARGO_PKG_VERSION`, never a literal, so no test changes are needed for the bump.
- `release.yml` triggers on `push: tags: ['v[0-9]+.[0-9]+.[0-9]+*']`, on `pull_request` with `branches: [main]` and `paths: ['.github/workflows/release.yml']` (build-only), and on `workflow_dispatch` (build-only). Its default permissions are `contents: read`. Only `publish` has `contents: write`, and it runs only when `startsWith(github.ref, 'refs/tags/v')`.
- Jobs run in the order `gate` (`uses: ./.github/workflows/ci.yml`) → `verify` → `build` → `publish`.
- The `build` matrix is exactly `x86_64-unknown-linux-musl` and `x86_64-unknown-linux-gnu`, with identical steps except a conditional `musl-tools` install. There is no aarch64 entry.
- `verify` fails on tag refs when the tag minus its leading `v` differs from the workspace version. In build-only mode it skips the comparison but prints the version.
- `build`:
  - runs `cargo build --release --locked -p baton --target $T`;
  - smoke-tests `--version` (equal to the version) and `BATON_NO_UPDATE_CHECK=1 baton version`;
  - asserts `file` reports `statically linked` for musl;
  - packages `baton-<tag-or-sha>-<target>.tar.gz` containing `baton`, `LICENSE` and `README.md` under one top-level dir, with a `.sha256` file;
  - uploads both as workflow artifacts.
- `publish`:
  - downloads all artifacts and writes `SHA256SUMS` over the archives;
  - runs `gh release create "$GITHUB_REF_NAME" dist/* --verify-tag --title "$GITHUB_REF_NAME" --generate-notes`, adding `--prerelease` when the tag contains `-`;
  - uses no third-party release action.
- `[profile.release]` sets `strip = "symbols"`, `lto = "thin"` and `codegen-units = 1`. The gate (debug builds) is unaffected.
- `docs/RELEASING.md` documents:
  - the bump / PR to `main` / tag / push steps and the tag/version check;
  - the artifact names and verification (`sha256sum -c SHA256SUMS --ignore-missing`);
  - which archive to pick: musl is recommended; gnu needs glibc ≥ 2.39;
  - installing to `~/.local/bin`;
  - `BATON_NO_UPDATE_CHECK` and `update_check = false`;
  - a **note that the repo must be made public before the update check and public downloads work**. Until then, `baton version --check` reports `no public release found`, doctor shows a WARN, and the TUI shows nothing, the same as offline;
  - a license note: PolyForm Noncommercial 1.0.0, source-available and not open source, `LICENSE` shipped in every archive.
- `actionlint` reports no errors.
Evidence:
- `actionlint` exits 0.
- The PR from Task 1, once it contains `release.yml`, triggers the build-only run. `gh run watch <id> --exit-status` exits 0, and `gh run download <id> -D $D/art && (cd $D/art && for f in */*.sha256; do (cd $(dirname $f) && sha256sum -c $(basename $f)); done)` prints `OK` for both archives.
- `tar tzf …musl.tar.gz` lists `baton`, `LICENSE` and `README.md`. After extracting, `./baton-*/baton --version` prints `baton 0.2.0`.
- After merge and `git tag -a v0.2.0 && git push origin v0.2.0`, `gh release view v0.2.0 --json assets -q '.assets[].name'` lists 2 archives, 2 `.sha256` files and `SHA256SUMS`.
- Once the repo is public, `baton version --check` reports `baton 0.2.0 is up to date`.

## Questions for the user
None.
