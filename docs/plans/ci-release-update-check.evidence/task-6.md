# Evidence — Task 6: MIT LICENSE and README
Commit: 2423db4
Environment: local checkout, debug build of `baton`, scratch HOME/XDG dirs under mktemp outside the repo. Network reachable (api.github.com returns 404 for releases/latest: no release yet).

## LICENSE is the standard MIT text, first line `MIT License`, `Copyright (c) 2026 ga-lep`
Status: PROVEN
```console
$ head -1 LICENSE; grep -n Copyright LICENSE
MIT License
3:Copyright (c) 2026 ga-lep
$ gh api /licenses/mit --jq .body | sed 's/\[year\]/2026/;s/\[fullname\]/ga-lep/' > mit.txt; diff mit.txt LICENSE
22d21
< 
```
Only difference from the canonical GitHub text is a trailing blank line in the API body; the notice and disclaimer through "OTHER DEALINGS IN THE SOFTWARE." are identical.

## Cargo.toml unchanged, license.workspace on all four crates
Status: PROVEN
```console
$ git diff --stat b54f59f..2423db4 -- Cargo.toml crates/*/Cargo.toml | wc -c
0
$ grep -c 'license.workspace = true' crates/*/Cargo.toml
crates/baton/Cargo.toml:1
crates/baton-proto/Cargo.toml:1
crates/baton-testkit/Cargo.toml:1
crates/baton-core/Cargo.toml:1
$ grep -n license Cargo.toml
8:license = "MIT"
```

## cargo metadata prints exactly MIT
Status: PROVEN
```console
$ cargo metadata --no-deps --format-version 1 | jq -r '.packages[].license' | sort -u
MIT
```

## README content
Status: PROVEN
README.md (full read) has: a description paragraph; `## Install` (musl archive from https://github.com/ga-lep/baton/releases, `sha256sum -c <checksum file>`, `~/.local/bin`, "See `docs/RELEASING.md`"); `## Updates` with `baton version --check`, `BATON_NO_UPDATE_CHECK=1` and `update_check = false`; `## License` "Baton is licensed under the MIT License. See `LICENSE`." It is 28 lines.
Documented commands run today:
```console
$ baton version --check      # scratch HOME/XDG
could not check for updates: no public release found
exit=1
$ BATON_NO_UPDATE_CHECK=1 baton doctor --no-probe
PASS config: /tmp/bLMvP/c/baton/config.toml parses
PASS dirs: runtime dir /tmp/bLMvP/x/baton is private (0700)
PASS dirs: state dir /tmp/bLMvP/s/baton is writable
WARN daemon: not running
PASS notify: notify-send and the D-Bus session bus are available
PASS version: 0.1.0 (update check disabled)
exit=0
```
Notes: the first `version --check` attempt returned `could not check for updates: timeout: global` (transient; two reruns gave the "no public release found" line above). Without the env var, doctor showed `WARN version: could not check for updates (last check failed)`. `docs/RELEASING.md` does not exist yet (`ls` -> No such file); it is a Task 7 deliverable, so the README pointer dangles for now.

## The gate still passes
Status: PROVEN
```console
$ cargo fmt --check; echo fmt=$?
fmt=0
$ cargo clippy --all-targets --all-features -- -D warnings   # tail
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s
clippy=0
$ cargo test 2>&1 | grep -c FAILED
0
```
All `test result` lines were `ok`.
