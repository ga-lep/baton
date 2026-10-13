# Evidence — Task 5: README and example config docs
Commit: b844b65
Environment: cargo build (debug, target dir in scratchpad), `baton config check` run with BATON_CONFIG; cargo test full workspace.

## README gains a "Copying text from a session" section covering all listed points
Status: PROVEN

```console
$ grep -n fullscreen README.md; sed -n 40,59p README.md
(README.md lines 40-59, section "## Copying text from a session")
- z / Alt-z toggle fullscreen; Esc in normal mode also leaves it (l.45-46)
- no border or sidebar, mouse capture off, plain drag selects (l.47-49)
- bottom bar stays, stop the drag above it (l.50-51)
- Claude receives no mouse events while fullscreen (l.52)
- soft-wrapped lines copy as separate lines (l.54-55)
- remappable as `fullscreen` under [keybindings.normal]/[keybindings.focus]; setting it replaces defaults (l.57-59)
```
Every required point is present in the section (read directly from the file; the above is an index of the text, which was printed in full during the run).

## examples/config.toml shows fullscreen entries and still parses
Status: PROVEN

```console
$ grep -n fullscreen examples/config.toml
53:# Fullscreen session pane. Setting it replaces the default keys (z, alt-z),
55:fullscreen = ["z", "alt-z"]
61:fullscreen = "alt-z"
$ BATON_CONFIG=examples/config.toml baton config check; echo "exit=$?"
backend  /home/glepape/code/api  profile=work  cmd=["claude"]  env=
backend  /home/glepape/code/worker  profile=work  cmd=["claude"]  env=
backend  /home/glepape/code/infra  profile=personal  cmd=["claude"]  env=CLAUDE_CONFIG_DIR
blog  /home/glepape/code/blog  profile=personal  cmd=["claude"]  env=CLAUDE_CONFIG_DIR
exit=0
$ printf '[keybindings.normal]\nfullscreen = ["z", "alt-z"]\n' > c.toml; BATON_CONFIG=c.toml baton config check; echo "exit=$?"
exit=0
```
Line 55 is under [keybindings.normal], line 61 under [keybindings.focus]; both configs pass check.

## cargo test is green
Status: PROVEN

```console
$ cargo test 2>&1 | grep -E "^test result|FAILED|failed"
test result: ok. 195 passed; 0 failed; ...
(all further "test result" lines: ok, 0 failed; remaining matches were test names containing "failed")
```
No failing result lines.

## Verdict
EVIDENCE: PROVEN
