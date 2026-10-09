# Evidence — Task 2: update-check logic, cache and config key
Commit: 6d00b18
Environment: local cargo, `cargo test -p baton-core`, `target/debug/baton config check` with scratch BATON_CONFIG files outside the repo.

## compare / parse_release / Cache / is_fresh / enabled (unit-tested)
Status: PROVEN
```console
$ cargo test -p baton-core update::
test update::tests::freshness_rules ... ok
test update::tests::compare_unknown ... ok
test update::tests::compare_table ... ok
test update::tests::parse_release_requires_tag_name ... ok
test update::tests::enabled_rules ... ok
test update::tests::parse_release_extracts_fields ... ok
test update::tests::cache_load_tolerates_bad_files ... ok
test update::tests::cache_round_trips_and_creates_parent ... ok
test result: ok. 8 passed; 0 failed
```
All 8 update tests pass, covering each listed rule by name. Case contents were not individually inspected here; this is a library-only surface (exercised externally in Task 3).

## paths::update_cache_path() returns <state_dir>/update-check.json
Status: PROVEN
```console
$ cargo test -p baton-core   (filtered)
test result: ok. 105 passed; 0 failed   (includes paths::tests::update_cache_lives_in_state_dir)
```
Source: `Ok(state_dir()?.join("update-check.json"))` in crates/baton-core/src/paths.rs:181.

## Config: update_check bool, default true; unknown key rejected; "yes" is an error naming the key
Status: PROVEN
```console
$ cargo test -p baton-core update_check   (config_examples)
test update_check_must_be_a_bool ... ok
test update_check_defaults_on_and_can_be_disabled ... ok
$ cargo test -p baton-core unknown
test unknown_top_level_key_still_rejected ... ok
$ BATON_CONFIG=scratch/a.toml baton config check      # update_check = false
exit=0
$ BATON_CONFIG=scratch/b.toml baton config check      # update_check = "yes"
invalid config: TOML parse error at line 1, column 16
  |
1 | update_check = "yes"
  |                ^^^^^
invalid type: string "yes", expected a boolean

exit=1
$ BATON_CONFIG=scratch/c.toml baton config check      # bogus_key = 1
invalid config: TOML parse error at line 1, column 1
  |
1 | bogus_key = 1
  | ^^^^^^^^^
unknown field `bogus_key`, expected one of `editor`, ..., `statusline`, `update_check`, `profiles`, ...
exit=1
```
false is accepted (exit 0); "yes" is rejected (exit 1) and the key is shown via the quoted source line (the message text itself does not repeat the key name); unknown keys are still rejected and `update_check` is now listed as valid.

## No network code and no tokio in baton-core
Status: PROVEN
```console
$ grep -n "tokio\|reqwest\|ureq\|hyper" crates/baton-core/Cargo.toml
(no output)
```
Dependencies are only baton-proto, nix, serde, semver, serde_json, shell-words, shellexpand, thiserror, toml.
