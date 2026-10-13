# Evidence — Fullscreen session pane (whole feature)
Commit: b3c4a04
Environment: debug build (`cargo build --workspace --bins`), real `baton` TUI + daemon with `fake-claude` as the profile command, 40x120 pty. Scratch config/state in the session scratchpad, runtime dir /tmp/bf1 (short, SUN_LEN). Env: BATON_CONFIG/STATE_DIR/RUNTIME_DIR, BATON_NOTIFY_SINK=off, BATON_NO_UPDATE_CHECK=1. Screens via `baton-drive`; raw terminal bytes via a Python pty harness (120x40) driving the same flow. Screens are trimmed to the first 5 rows plus the bar row (awk), trailing spaces removed.

## Goal flow: z -> borderless, Claude text at col 0, NORMAL bar with `z fullscreen`; Esc -> normal layout; Enter + Alt-z -> FOCUS bar with `Alt-z fullscreen`; typed text reaches session; Alt-z back
Status: PROVEN

```console
$ baton-drive --size 40x120 --step 'wait:▸ x' --step 'send:o' --step 'wait:FAKE CLAUDE session=' --step 'sleep:400' --step dump --step 'send:z' --step 'sleep:400' --step dump --step 'send:\e' --step 'sleep:400' --step dump --step 'send:\r' --step 'sleep:300' --step 'send:\ez' --step 'sleep:400' --step dump --step 'send:hi there\r' --step 'sleep:500' --step dump --step 'send:\ez' --step 'sleep:400' --step dump ... -- baton
--- screen 1: session opened (normal layout)
┌Projects──────────────────────┐┌alpha · p · ○ idle──────────...──┐
│▾ x                           ││DA1: ok                          ...
│  1 ○ alpha  idle             ││FAKE CLAUDE session=ba885762-f08e-42e6-9589-9a4b7c8832ff model=claude-opus-5-5
│  2 ○ beta  idle              ││>
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit
--- screen 2: after z
DA1: ok
FAKE CLAUDE session=ba885762-f08e-42e6-9589-9a4b7c8832ff model=claude-opus-5-5
>


 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit
--- screen 3: after Esc
┌Projects──────────────────────┐┌alpha · p · ○ idle──────────...──┐
│▾ x                           ││DA1: ok
│  1 ○ alpha  idle             ││FAKE CLAUDE session=ba885762-...
│  2 ○ beta  idle              ││>
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit
--- screen 4: after Enter, Alt-z
DA1: ok
FAKE CLAUDE session=ba885762-f08e-42e6-9589-9a4b7c8832ff model=claude-opus-5-5
> █


 FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session  Alt-z fullscreen
--- screen 5: after typing "hi there\r"
DA1: ok
FAKE CLAUDE session=ba885762-f08e-42e6-9589-9a4b7c8832ff model=claude-opus-5-5
> hi there
> █

 FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session  Alt-z fullscreen
--- screen 6: after Alt-z
┌Projects──────────────────────┐┌alpha · p · ○ idle──────────...──┐
│▾ x                           ││DA1: ok
│  1 ○ alpha  idle             ││FAKE CLAUDE session=ba885762-...
│  2 ○ beta  idle              ││> hi there
 FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session  Alt-z fullscreen
```
Screens 2 and 4 have no sidebar, border or title; `FAKE CLAUDE session=` is at column 0 of row 1; the last row is the bar with ` NORMAL │ ` + `z fullscreen` (normal) and ` FOCUS │ ` + `Alt-z fullscreen` (focus). Esc restores the normal layout (3). Text typed in fullscreen focus echoes from the fake session (`> hi there`, 5), so it reached the session, and Alt-z restores the layout (6) while staying in FOCUS mode.

## Mouse reporting off in fullscreen, on again after; 1007 reset/restored; terminal state restored on quit
Status: PROVEN

Raw bytes written by baton, filtered to mouse/alt-scroll/alt-screen DECSET/DECRST, per key phase. (The first row is labelled "startup" but the startup bytes arrived with the next read, so they show in the `o` row.)
```console
$ python3 rec.py   # pty 120x40, runs baton, sends keys, prints ?NNNN[hl] sequences seen after each
startup                                    []
o (open project)                           ['?1049h', '?1000h', '?1002h', '?1003h', '?1015h', '?1006h']
z (fullscreen, normal)                     ['?1006l', '?1015l', '?1003l', '?1002l', '?1000l', '?1007l']
Esc (leave)                                ['?1000h', '?1002h', '?1003h', '?1015h', '?1006h', '?1007h']
Enter (focus)                              []
Alt-z (fullscreen, focus)                  ['?1006l', '?1015l', '?1003l', '?1002l', '?1000l', '?1007l']
type 'hi there\r'                          []
Alt-z (leave)                              ['?1000h', '?1002h', '?1003h', '?1015h', '?1006h', '?1007h']
Alt-z (fullscreen again)                   ['?1006l', '?1015l', '?1003l', '?1002l', '?1000l', '?1007l']
Ctrl-\ (unfocus, stays fullscreen)         []
q (quit while fullscreen)                  ['?1007h', '?1006l', '?1015l', '?1003l', '?1002l', '?1000l', '?1049l']
```
Mouse modes are enabled at start, disabled with `?1007l` on each fullscreen entry (z and Alt-z), re-enabled with `?1007h` on each exit, untouched by Enter/typing/Ctrl-\ (which does not leave fullscreen). Quitting while fullscreen emits `?1007h` before disabling mouse and leaving the alt screen, restoring the terminal's state. Mouse reporting off in the vt100 model is additionally asserted by the e2e test below.

## Task 1 Evidence: `config check` keymap override / conflict
Status: PROVEN
```console
$ BATON_CONFIG=c.toml baton config check   # [keybindings.normal] fullscreen = "f"
exit=0
$ BATON_CONFIG=c.toml baton config check   # [keybindings.focus] fullscreen = "f12"
exit=0
$ BATON_CONFIG=c.toml baton config check   # [keybindings.focus] fullscreen = "alt-n"
invalid config: keybindings.focus: key "alt-n" is bound to both next_attention and fullscreen
exit=1
```

## Task 2 Evidence: none (reducer state only)
Status: NOT PROVEN (internal — covered by tests; behaviour exercised above via the goal flow)

## Task 3 Evidence: dump with no sidebar/borders, `FAKE CLAUDE session=` at col 0, last row ` NORMAL │ ` with `z fullscreen`
Status: PROVEN — see screen 2 above (and focus variant, screen 4).

## Task 4 Evidence: e2e test
Status: PROVEN
```console
$ cargo test -p baton --test e2e_keys fullscreen
running 1 test
test fullscreen_drops_borders_and_mouse_capture_and_restores_them ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.47s
```
Full `cargo test --workspace` also run: every `test result` line is `ok`, no failures.

## Task 4 Evidence (manual): plain mouse drag selects only Claude text, no `│`/sidebar in the paste
Status: NOT PROVEN (manual, for the user). A real mouse drag in a real terminal emulator cannot be automated here. The byte capture above shows baton turns mouse reporting and alternate scroll off in fullscreen, which is the precondition. Stop the drag above the bottom bar.

## Task 5 Evidence: example config check and docs entries
Status: PROVEN
```console
$ BATON_CONFIG=examples/config.toml baton config check
backend  /home/glepape/code/api  profile=work  cmd=["claude"]  env=
backend  /home/glepape/code/worker  profile=work  cmd=["claude"]  env=
backend  /home/glepape/code/infra  profile=personal  cmd=["claude"]  env=CLAUDE_CONFIG_DIR
blog  /home/glepape/code/blog  profile=personal  cmd=["claude"]  env=CLAUDE_CONFIG_DIR
exit=0
$ grep -n fullscreen README.md examples/config.toml
README.md:43:and borders would end up in the copy. Use fullscreen instead:
README.md:46:  fullscreen. `Esc` in normal mode also leaves it.
README.md:50:- The bottom bar stays, showing the mode and the fullscreen key. Stop the drag
README.md:52:- Claude receives no mouse events while fullscreen (no wheel scroll or clicks).
README.md:57:- The keys are remappable as the `fullscreen` action under `[keybindings.normal]`
README.md:58:  and `[keybindings.focus]`. Setting `fullscreen` replaces that mode's default
examples/config.toml:55:fullscreen = ["z", "alt-z"]
examples/config.toml:61:fullscreen = "alt-z"
```
(Observation: the example config shows `fullscreen = ["z", "alt-z"]` for normal, not the plan's literal `fullscreen = "z"`; it parses and expresses the defaults. The README lines fit the plan's required topics except soft-wrapping, which was not checked line by line in this grep.)

## Cleanup
`baton daemon stop` run (daemon and fake-claude sessions stopped), /tmp/bf1 and scratch dirs removed, only this file added. Pids 10103 and 966758 were not touched and are still running.

## Verdict
EVIDENCE: PROVEN (real mouse-drag selection NOT PROVEN, manual)
