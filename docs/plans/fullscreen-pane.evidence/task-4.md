# Evidence — Task 4: mouse capture off in fullscreen
Commit: 3d3f842
Environment: debug build of `baton` with `fake-claude` as the session command. A Python pty harness (120x40 pty, scratchpad `rec.py`/`rec2.py`) drove a real `baton` process and recorded the bytes it wrote. Private 0700 runtime and state dirs under /tmp/bt4, removed afterwards. The harness's own baton daemon and fake-claude were stopped. The user's pids 10103 and 966758 were not touched.

## e2e: open project -> z (borderless, NORMAL bar, mouse off) -> Esc -> Enter, Alt-z (FOCUS bar, mouse off) -> `hi\r` reaches session -> Alt-z restores
Status: PROVEN
```console
$ cargo test -p baton --test e2e_keys fullscreen
running 1 test
test fullscreen_drops_borders_and_mouse_capture_and_restores_them ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.45s
```
The test asserts the exact bar text (` NORMAL │ ` with `z fullscreen`, ` FOCUS │ ` with `Alt-z fullscreen`), `Drive::mouse_reporting()` false in fullscreen and true when windowed, and `hi` appearing in the session. The test was only read, not extended. Its assertions are the ones listed in the criterion.

## `set_mouse_capture(out, false)` writes DisableMouseCapture + `\x1b[?1007l`; `true` writes EnableMouseCapture + `\x1b[?1007h`; `restore()` writes `?1007h` only if still reset
Status: PROVEN
Raw bytes from the real binary, filtered to the mouse/alt-scroll/alt-screen DECSET/DECRST sequences per phase (`ESC` = 0x1b):
```console
$ python3 (regex over raw.bin split at harness markers)
'<startup>'                      ['ESC[?1049h', 'ESC[?1000h', 'ESC[?1002h', 'ESC[?1003h', 'ESC[?1015h', 'ESC[?1006h']
"open project 'o'"               []
'z (fullscreen, normal mode)'    ['ESC[?1006l', 'ESC[?1015l', 'ESC[?1003l', 'ESC[?1002l', 'ESC[?1000l', 'ESC[?1007l']
'z (leave)'                      ['ESC[?1000h', 'ESC[?1002h', 'ESC[?1003h', 'ESC[?1015h', 'ESC[?1006h', 'ESC[?1007h']
'z (fullscreen again)'           ['ESC[?1006l', 'ESC[?1015l', 'ESC[?1003l', 'ESC[?1002l', 'ESC[?1000l', 'ESC[?1007l']
'q (quit in fullscreen)'         ['ESC[?1007h', 'ESC[?1006l', 'ESC[?1015l', 'ESC[?1003l', 'ESC[?1002l', 'ESC[?1000l', 'ESC[?1049l']
```
Entering fullscreen: the crossterm disable set followed by `?1007l`. Leaving: the enable set followed by `?1007h`. Quitting while still in fullscreen: `?1007h` is written once on restore, then the mouse modes are disabled and the alt screen is left. An earlier run that went fullscreen -> windowed -> Alt-z focus fullscreen -> windowed (the focused path via `Alt-z`) showed the same off/on pairs for each toggle. Unit tests for the exact byte sequences are in terminal_guard.rs and were not run separately here. They are covered by the byte-level capture above.

## `drive` keeps a `captured` flag and calls `set_mouse_capture` only when `wants_mouse_capture() != captured`
Status: PROVEN (observed)
The capture above shows exactly one disable at each entry into fullscreen and one enable at each exit, with nothing emitted on the other keypresses or redraws (the `[]` phases). Code: `capture_change(wants, captured)` in crates/baton/src/tui/event_loop.rs, with `captured` starting at `true`.

## `Drive::mouse_reporting()` reports whether vt100 sees a mouse protocol mode
Status: PROVEN
The e2e test above uses it to distinguish the fullscreen and windowed states and passes.

## Manual: a plain mouse drag in a real terminal selects only Claude's text after Alt-z
Status: NOT PROVEN (manual, for the user). A real mouse drag cannot be performed here. The bytes above show that the terminal is told to stop mouse reporting and alternate scroll on entering fullscreen.

## Cleanup
Harness-started baton daemon and fake-claude were stopped, /tmp/bt4 removed, and `git status` is clean apart from this file. Pids 10103 and 966758 are still running.
