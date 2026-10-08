# Evidence — Task 15: Desktop notifications (M3b)
Commit: e20d22b
Environment: target/debug/baton with BATON_NOTIFY_SINK=log (real D-Bus sink never used, no dbus-monitor), scratch BATON_CONFIG/BATON_STATE_DIR/BATON_RUNTIME_DIR/FAKE_CLAUDE_HOME under a short mktemp dir `$D` (/tmp/bt.XXXX; a short path was needed because of the unix socket SUN_LEN limit). Profile command = target/debug/fake-claude; project `x` with repos alpha and beta. TUI driven with target/debug/baton-drive (40x120 PTY). Session ids look like `x//tmp/bt.XZWW/alpha`. Output below is trimmed to the relevant lines (drive dump filtered with grep).

## Sink / Evidence line: LogSink via BATON_NOTIFY_SINK=log writes `notify <session> <status>` to <state>/notifications.log; with no TUI attached, `perm` produces one line
Status: PROVEN
```console
$ baton daemon start; baton debug open x; baton debug sessions --json | jq -c '.[]|[.id,.status]'
daemon started pid=1207504
x//tmp/bt.XZWW/alpha
x//tmp/bt.XZWW/beta
["x//tmp/bt.XZWW/alpha","Idle"]
["x//tmp/bt.XZWW/beta","Idle"]
$ baton debug send "x/$D/alpha" 'perm\r'; sleep 1; cat $D/state/notifications.log
notify x//tmp/bt.XZWW/alpha Permission
```
One line in the log, in the specified format.

## Repeats: no repeat while the session remains in the same attention state; a new one after leaving and re-entering
Status: PROVEN
```console
$ sleep 1.5; cat $D/state/notifications.log          # still in Permission
notify x//tmp/bt.XZWW/alpha Permission
$ baton debug send "x/$D/alpha" 'allow\r'; sleep 1; cat $D/state/notifications.log
notify x//tmp/bt.XZWW/alpha Permission
notify x//tmp/bt.XZWW/alpha YourTurn
$ baton debug send "x/$D/alpha" 'perm\r'; sleep 1; cat $D/state/notifications.log
notify x//tmp/bt.XZWW/alpha Permission
notify x//tmp/bt.XZWW/alpha YourTurn
notify x//tmp/bt.XZWW/alpha Permission
```
No extra line while remaining in Permission; `allow` gives the "your turn" line; re-entering Permission gives a new line.

## Focus reporting / e2e: TUI attached, session on screen and focused (ESC[I) -> no line; focus lost (ESC[O) -> line; allow on an off-screen session -> your-turn line
Status: PROVEN
Fresh state. TUI: `o`, Enter, Ctrl-\ (NORMAL), then `ESC[I`; alpha on screen. Injector ran `baton debug send` in the background and printed the log after each step.
```console
$ baton-drive --size 40x120 --step 'send:o' --step 'wait:1 ○ alpha  idle' --step 'send:\r' --step 'wait:FOCUS' --step 'send:\x1c' --step 'wait:NORMAL' --step 'send:\x1b[I' --step 'sleep:11500' --step 'send:\x1b[O' --step 'sleep:8000' --step dump --step 'send:q' --step 'expect-exit:0' -- baton   (+ background injector)
[inj] perm alpha (on screen, focused)
--- log @6s:
cat: /tmp/bt.XZWW/state/notifications.log: No such file or directory
[inj] allow alpha
--- log @8s:
cat: /tmp/bt.XZWW/state/notifications.log: No such file or directory
[inj] perm beta (off-screen)
--- log @10s:
notify x//tmp/bt.XZWW/beta Permission
[inj] allow beta (off-screen)
--- log @12s:
notify x//tmp/bt.XZWW/beta Permission
notify x//tmp/bt.XZWW/beta YourTurn
[inj] ESC[O sent by drive next
[inj] perm alpha (unfocused)
--- log @18s:
notify x//tmp/bt.XZWW/beta Permission
notify x//tmp/bt.XZWW/beta YourTurn
notify x//tmp/bt.XZWW/alpha Permission
```
The TUI screen dump (filtered) showed `1 ◐ alpha  permission` at the end and `NORMAL` mode. Focused + on screen alpha: no log file at all, even after perm and allow. Off-screen beta: Permission and YourTurn lines. After ESC[O, alpha `perm` produced a line. The TUI exited with code 0 on `q`.
The TUI enabling focus reporting is at crates/baton/src/tui/terminal_guard.rs (`EnableFocusChange`); behaviourally it is shown by the ESC[I / ESC[O effects above.

## Config: `notifications = false` suppresses all notifications
Status: PROVEN
```console
$ BATON_CONFIG=$D/off.toml baton daemon start; baton debug open x; baton debug send "x/$D/alpha" 'perm\r'; sleep 2
daemon started pid=1210759
$ baton debug sessions --json | jq -c '.[]|[.id,.status]'
["x//tmp/bt.XZWW/alpha","Permission"]
["x//tmp/bt.XZWW/beta","Idle"]
$ ls $D/state; cat $D/state/notifications.log
daemon.log
cat: /tmp/bt.XZWW/state/notifications.log: No such file or directory
```
Session entered Permission with no TUI attached (which notifies when enabled), yet no log was written.

## `should_notify` pure with table tests; DbusSink (notify-rust, app name Baton, summaries); D-Bus failures logged and never propagate
Status: NOT PROVEN (internal — covered by tests; real D-Bus sink deliberately not exercised)

## Cleanup
```console
$ baton daemon stop
daemon stopped
$ pgrep -x baton; echo pgrep=$?
pgrep=1
```
Daemon stopped after each run; no baton process is left. `git status --short` in the repo showed nothing before this file was written. Scratch dir /tmp/bt.XZWW was removed. One earlier failed attempt left an empty-ish scratch dir under the session scratchpad (outside the repo).

## Verdict
EVIDENCE: PROVEN
