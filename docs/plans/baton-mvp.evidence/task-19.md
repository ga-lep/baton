# Evidence — Task 19: `baton doctor` (hook contract check)
Commit: cf13e48
Environment: target/debug/baton and fake-claude (cargo build). Scratch BATON_CONFIG/BATON_STATE_DIR/BATON_RUNTIME_DIR under /tmp/bd.4Ecv, BATON_NOTIFY_SINK=off, TMPDIR=<scratch>/tmp for the fake run. Real claude 2.1.295 (Claude Code). The script prints "exit=N" after each command.

## Checks 1-6 printed as PASS/WARN/FAIL lines; fake-claude profile -> hooks PASS, exit 0; --no-probe skips check 5; NO_HOOKS -> FAIL with hint, exit 1; daemon PASS with protocol version
Status: PROVEN

```console
$ baton doctor
PASS config: /tmp/bd.4Ecv/config.toml parses
PASS dirs: runtime dir /tmp/bd.4Ecv/run is private (0700)
PASS dirs: state dir /tmp/bd.4Ecv/state is writable
WARN daemon: not running
PASS command: /home/glepape/project/baton/target/debug/fake-claude (profile p)
PASS version: DA1: timeout (profile p)
PASS hooks: SessionStart received (profile p)
PASS notify: notify-send and the D-Bus session bus are available
exit=0
$ baton doctor --no-probe
PASS config: /tmp/bd.4Ecv/config.toml parses
PASS dirs: runtime dir /tmp/bd.4Ecv/run is private (0700)
PASS dirs: state dir /tmp/bd.4Ecv/state is writable
WARN daemon: not running
PASS command: /home/glepape/project/baton/target/debug/fake-claude (profile p)
PASS version: DA1: timeout (profile p)
PASS notify: notify-send and the D-Bus session bus are available
exit=0
$ baton doctor (NO_HOOKS)
PASS config: /tmp/bd.4Ecv/config.toml parses
PASS dirs: runtime dir /tmp/bd.4Ecv/run is private (0700)
PASS dirs: state dir /tmp/bd.4Ecv/state is writable
WARN daemon: not running
PASS command: /home/glepape/project/baton/target/debug/fake-claude (profile p)
PASS version: DA1: timeout (profile p)
FAIL hooks: no SessionStart within 20s: trust dialog pending or hooks disabled (disableAllHooks / --bare)? (profile p)
PASS notify: notify-send and the D-Bus session bus are available
exit=1
$ baton daemon start
daemon started pid=2254981
exit=0
$ baton doctor (daemon running)
PASS config: /tmp/bd.4Ecv/config.toml parses
PASS dirs: runtime dir /tmp/bd.4Ecv/run is private (0700)
PASS dirs: state dir /tmp/bd.4Ecv/state is writable
PASS daemon: running pid=2254981 protocol=5
PASS command: /home/glepape/project/baton/target/debug/fake-claude (profile p)
PASS version: DA1: timeout (profile p)
PASS hooks: SessionStart received (profile p)
PASS notify: notify-send and the D-Bus session bus are available
exit=0
$ ls tmp
daemon stopped
$ stat runtime
700 /tmp/bd.4Ecv/run
```
Every check prints its line. The healthy run exits 0 with `PASS hooks: SessionStart received (profile p)`. `--no-probe` drops the hooks line. NO_HOOKS gives `FAIL hooks` with the hint and exit 1. The daemon line is `WARN` when stopped and `PASS ... protocol=5` when running. The runtime dir is mode 700. The probe's TMPDIR was empty afterwards (`ls tmp` printed nothing). Note: the fake-claude `version:` line reads "DA1: timeout" because fake-claude does not implement `--version` (cosmetic, not a criterion failure).

## Real claude (no tokens): version captured; hook probe on trusted repos
Status: PROVEN for check 4 (command + `--version`). Check 5 FAILED for both profiles, recorded honestly below.

```console
$ BATON_CONFIG=real.toml baton doctor   # work=claude, personal=claude + CLAUDE_CONFIG_DIR=~/.claude-personal, repo = scratchpad dir
PASS config: /tmp/bd.4Ecv/real.toml parses
PASS dirs: runtime dir /tmp/bd.4Ecv/run is private (0700)
PASS dirs: state dir /tmp/bd.4Ecv/state is writable
WARN daemon: not running
PASS command: /home/glepape/.local/bin/claude (profile work)
PASS version: 2.1.295 (Claude Code) (profile work)
FAIL hooks: no SessionStart within 20s: trust dialog pending or hooks disabled (disableAllHooks / --bare)? (profile work)
PASS command: /home/glepape/.local/bin/claude (profile personal)
PASS version: 2.1.295 (Claude Code) (profile personal)
FAIL hooks: no SessionStart within 20s: trust dialog pending or hooks disabled (disableAllHooks / --bare)? (profile personal)
PASS notify: notify-send and the D-Bus session bus are available
exit=1
```
The version string `2.1.295 (Claude Code)` is captured for both profiles. The hooks probe timed out for both profiles, with the specified hint and exit 1. The most likely cause is that the scratchpad dir is not trusted for these profiles, or that SessionStart does not fire before a prompt. I did not investigate further, answer dialogs, or modify any Claude settings. This is not proven as a defect: the failure path and hint behave as specified, but the real-claude PASS hooks result was not obtained.

## Cleanup
```console
$ pgrep -af -- '--settings .*hooks.json'   # only the pgrep's own shell matched
$ pgrep -ax baton ; pgrep -ax fake-claude
(empty)
```
No doctor temp dir remained (/tmp diff before/after the real run showed only unrelated runc-process dirs removed). No probe claude process remained. The daemon was stopped ("daemon stopped").

## Not checked individually
"Probe never sends a prompt" is an internal property. It was observed only in that the real-claude run ended with no prompt sent.
