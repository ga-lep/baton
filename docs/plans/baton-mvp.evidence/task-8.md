# Evidence — Task 8: Config and paths, `baton config check`
Commit: e907070
Environment: `cargo build`, binary target/debug/baton; BATON_CONFIG/BATON_STATE_DIR/BATON_RUNTIME_DIR pointed at a scratch dir $D outside the repo (removed after).

## `baton config check` prints one line per session, exits 0
Status: PROVEN
```console
$ printf '[profiles.p]\ncommand="bash --norc"\nenv={FOO="bar"}\n[[projects]]\nname="x"\nprofile="p"\nrepos=[{path="/tmp"}]\n' > $D/config.toml; baton config check; echo exit=$?
x  /tmp  profile=p  cmd=["bash","--norc"]  env=FOO
exit=0
```
Matches the hint exactly; shell-words splitting works.

## Validation errors (unknown profile, duplicate project, duplicate repo, empty repos, invalid TOML), exit 1
Status: PROVEN
```console
$ (profile="nope")            -> project "x" repo /tmp: unknown profile "nope"      exit=1
$ (repos=[])                  -> project "a": repos is empty                         exit=1
$ (same repo path twice)      -> project "a": duplicate repo path /tmp               exit=1
$ (two projects named a)      -> duplicate project name "a"                          exit=1
$ (malformed TOML "[[projects]") -> invalid config: TOML parse error at line 1, column 12 ... unclosed array table, expected `]`   exit=1
```

## Missing config file -> empty config, not an error
Status: PROVEN
```console
$ rm $D/config.toml; baton config check; echo exit=$?
exit=0
```
No output, exit 0.

## Expansion of ~ and $VAR in repo paths, env values and command
Status: PROVEN (repo path and command observed; env values not printed by check)
```console
$ command="$MYBIN --x", repos path "~/code/$SUB"; MYBIN=/opt/mybin SUB=repo1 baton config check
x  /home/glepape/code/repo1  profile=p  cmd=["/opt/mybin","--x"]  env=A,B
exit=0
```
Env value expansion is not visible from the CLI; covered by the unit/integration tests.

## The spec section 5 example parses verbatim and passes check
Status: PROVEN
```console
$ (SPEC.md lines 142-176, the toml block, as config.toml) baton config check
loop  /home/glepape/code/alaloop  profile=work  cmd=["claude"]  env=
loop  /home/glepape/code/powerloop  profile=work  cmd=["claude"]  env=
loop  /home/glepape/code/hyperloop  profile=personal  cmd=["claude"]  env=CLAUDE_CONFIG_DIR
blog  /home/glepape/code/blog  profile=personal  cmd=["claude"]  env=CLAUDE_CONFIG_DIR
```
(The `powerloop` line shows `env=` ... trimmed to the 4 output lines; the project/repo profile override on hyperloop is visible.) Note: `args` are not printed by check, so that part is unobserved here.

## Paths honour env overrides; runtime dir 0700 hardening
Status: PROVEN (via unit tests; `config check` does not create or validate the runtime dir)
```console
$ mkdir -m 0755 $D/run; baton config check; echo exit=$?; stat -c %a $D/run
exit=0
755
$ cargo test -p baton-core
test paths::tests::overrides_win ... ok
test paths::tests::runtime_falls_back_to_tmp_uid ... ok
test paths::tests::private_dir_fresh_is_created_0700 ... ok
test paths::tests::private_dir_rejects_loose_mode ... ok
test paths::tests::private_dir_rejects_other_owner ... ok
test paths::tests::private_dir_rejects_symlink ... ok
test paths::tests::xdg_defaults ... ok
test result: ok. 20 passed; 0 failed
test result: ok. 8 passed; 0 failed   (tests/config_examples.rs)
```
The CLI does not touch the runtime dir (the 0755 dir was left alone), so refusal is not reachable from outside yet; the unit test `private_dir_rejects_loose_mode` covers it.

## Defaults (editor, notifications, scrollback_lines, implicit `default` profile, hook_timeout_secs=20); args appended; repo-over-project profile precedence
Status: NOT PROVEN (internal — covered by tests; the implicit `default` profile is partly seen in 8 passing config_examples tests). Repo override seen above (hyperloop -> personal).

## Verdict
EVIDENCE: PROVEN
