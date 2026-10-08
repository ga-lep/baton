# Evidence — Task 1: Workspace scaffold and CLI skeleton
Commit: dffe86d
Environment: clean export (`git archive HEAD`) into a scratch dir, built with cargo there; binary target/debug/baton. No server needed.

## The gate command passes on a clean checkout.
Status: PROVEN
```console
$ cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
... (tail) test cli::tests::help_lists_public_subcommands_but_not_debug ... ok
test result: ok. 5 passed; 0 failed (baton unit tests); all other crates/doc-tests 0 tests, ok
gate=0
```
All three gate steps exited 0 on the clean export.

## `baton --help` lists daemon, hook, spike, doctor, config; hidden `debug` not shown
Status: PROVEN
```console
$ baton --help
Commands:
  daemon  Manage the background daemon
  hook    Claude Code hook entry point; reads the event JSON on stdin
  spike   Run the embedding spike against a child command (defaults to `claude`)
  doctor  Check the local environment
  config  Inspect the configuration
  help    Print this message or the help of the given subcommand(s)
$ baton debug
exit=2 stdout=[] stderr=[not implemented yet]
```
The five commands are listed, `debug` is absent from help yet is accepted and runs.

## `baton daemon --help` lists start/stop/status; `baton hook --help` shows positional <EVENT>
Status: PROVEN
```console
$ baton daemon --help
Commands:
  start   Start the daemon
  stop    Stop the daemon
  status  Show daemon status
$ baton hook --help
Usage: baton hook <EVENT>
Arguments:
  <EVENT>  Hook event name, e.g. `Stop`
```

## Unimplemented subcommands print `not implemented yet` to stderr, exit 2; `baton hook` exits 0 silently
Status: PROVEN
```console
$ baton daemon start|stop|status ; baton spike ; baton doctor ; baton config check ; baton debug
each: exit=2 stdout=[] stderr=[not implemented yet]
$ echo '{}' | baton hook Stop
exit=0            (no output)
$ echo '{}' | cargo run -q -p baton -- hook Stop 2>&1; echo "exit=$?"
exit=0            (no output, stdout+stderr merged)
```
Note: bare `baton config` (no subcommand) exits 2 with clap usage text, which is a clap usage error, not the stub path.

## A unit test asserts the clap command passes `debug_assert()`.
Status: PROVEN
```console
$ grep -n debug_assert -B2 crates/baton/src/cli.rs
80:    fn clap_command_is_valid() {
81:        Cli::command().debug_assert();
```
The test exists and passed in the gate run (5 passed in the baton crate).
