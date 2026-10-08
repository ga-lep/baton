# Evidence — Task 11: TUI client M1
Commit: bdede1c
Environment: target/debug/baton + baton-drive, 40x120 PTY. Scratch dir $D (mktemp) with BATON_CONFIG/BATON_STATE_DIR/BATON_RUNTIME_DIR; profile `bash --norc --noprofile`, project x with one repo $D/a. `B=target/debug/baton`, `DR=target/debug/baton-drive`. Blank lines and long rows trimmed. $D removed afterwards.

## Evidence flow step 1: attach, Enter focuses, type, Ctrl-\ returns to NORMAL, q exits 0
Status: PROVEN
```console
$ B daemon start; B debug open x
daemon started pid=1036830
x//tmp/tmp.Y8DdR8N7bF/a
$ DR --size 40x120 --step 'wait:NORMAL' --step 'send:\r' --step 'wait:FOCUS' --step 'send:echo persisted-42\r' --step 'wait:persisted-42' --step 'send:\x1c' --step 'sleep:300' --step dump --step 'send:q' --step 'expect-exit:0' -- B
--- screen 40x120 ---  (trimmed)
┌Sessions──────────────────────┐┌x/a · starting────────────────────────────
│… x/a  starting               ││bash-5.2$ echo persisted-42
│                              ││persisted-42
│                              ││bash-5.2$
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  ? help  q quit
--- end screen ---
drive exit=0
```
FOCUS was reached, the typed command ran in the embedded main panel, Ctrl-\ gave NORMAL bar, q exited 0.

## Focus mode bar and highlighted main panel
Status: PROVEN (bar text); border highlight not checkable in the text dump (no colour info)
```console
 FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session
```

## Evidence step 2: daemon keeps the session after quit
Status: PROVEN
```console
$ B daemon status
running pid=1036830 protocol=1 sessions=1
```

## Evidence step 3: relaunch shows the screen intact
Status: PROVEN
```console
$ DR --size 40x120 --step 'wait:persisted-42' --step dump -- B
│… x/a  starting               ││bash-5.2$ echo persisted-42
│                              ││persisted-42
│                              ││bash-5.2$
 NORMAL │ ⏎ focus ...
exit=0
```
Same content as before quit (full dump identical in layout).

## Layout: flat session list, info panel, main panel, bottom bar NORMAL/FOCUS
Status: PROVEN
Dumps above show Sessions list, "Session" info panel (path, status), main panel, NORMAL and FOCUS bars.

## Daemon gone: banner, no panic
Status: PROVEN
```console
$ (DR ... --step 'sleep:4000' --step 'wait:daemon disconnected' --step dump --step 'send:q' --step 'expect-exit:0' -- B) &  ; B daemon stop
daemon stopped
│                              │daemon disconnected — press r to reconnect, q to quit  │
 NORMAL │ ⏎ focus ...
drive exit=0
```
Banner shown, q exits 0 with no panic output.

## Daemon gone: r reconnects (extra)
Status: PROVEN
After daemon stop then start + `debug open x`, pressing `r` in the TUI showed the session again (`x/a · starting`, `bash-5.2$`), then q exit 0.

## Quit leaves daemon/sessions running
Status: PROVEN (see step 2).

## 64 KiB Input cap
Status: NOT PROVEN (internal — covered by unit test crates/baton/src/tui/app/tests.rs, which asserts chunks <= MAX_INPUT; not exercised via the PTY)

## Event loop (60 fps, sync_output deferral), mirror never answers `ESC[c`, resize, mouse, focus in/out, version-mismatch modal
Status: NOT PROVEN (internal — covered by tests; not exercised here)

## Cleanup
```console
$ B daemon stop; pgrep -x baton; echo pgrep=$?
pgrep=1
```
No baton process left; repo clean apart from this file.
