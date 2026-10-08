# Evidence — Task 6: M0 embedding spike (`baton spike`)
Commit: 66570ab
Environment: `cargo build` debug; `target/debug/baton-drive` (PTY driver) running `target/debug/baton spike`, cwd = scratch dir, 40x120. Blank panel rows trimmed from dumps (lines of `│ ││ │`). Note: the hint's `wait:^86$` times out because the panel is bordered (the line is `│…││86 …│`), so `wait:│86 ` was used instead; the screen dump showed `86` in that run too.

## Command and layout (32-col sidebar, bordered main panel, bottom bar with mode and rows x cols)
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:FOCUS' --step 'send:tput cols\r' --step 'wait:│86 ' --step 'send:\x1c' --step 'wait:NORMAL' --step dump --step 'send:q' --step 'expect-exit:0' -- baton spike -- bash --norc
--- screen 40x120 ---
┌Baton─────────────────────────┐┌──────────────────────────────────────────────────────────────────────────────────────┐
│                              ││bash-5.2$ tput cols                                                                   │
│                              ││86                                                                                    │
│                              ││bash-5.2$                                                                             │
└──────────────────────────────┘└──────────────────────────────────────────────────────────────────────────────────────┘
 NORMAL | 37x86 | Enter: focus, q: quit
--- end screen ---
exit=0
```
(The first run, before Ctrl-\, showed ` FOCUS | 37x86 | Ctrl-\: normal mode`.) 32-col sidebar, bordered panel, 37x86 as specified.

## Focus mode and keys (starts in FOCUS; Ctrl-\ -> NORMAL not forwarded; q quits child, exit 0)
Status: PROVEN
Same run as above: started FOCUS, `\x1c` produced NORMAL (no stray byte in bash prompt), `q` -> `expect-exit:0` passed. Enter-to-refocus, legacy Char('4'), paste and mouse wheel: NOT PROVEN (internal — covered by tests), not exercised by driver.

## Rendering: resize resizes PTY and screen; PTY replies written back
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:37x86' --step 'send:printf "\e[31mRED\e[0m\n"\r' --step 'wait:│RED' --step 'send:tput cols\r' --step 'wait:│86 ' --step 'resize:30x100' --step 'wait:27x66' --step 'send:tput cols\r' --step 'wait:│66 ' --step dump --step 'send:\x1c' --step 'send:q' --step 'expect-exit:0' -- baton spike -- bash --norc
--- screen 30x100 ---
┌Baton─────────────────────────┐┌──────────────────────────────────────────────────────────────────┐
│                              ││bash-5.2$ printf "\e[31mRED\e[0m\n"                               │
│                              ││RED                                                               │
│                              ││bash-5.2$ tput cols                                               │
│                              ││86                                                                │
│                              ││bash-5.2$ tput cols                                               │
│                              ││66                                                                │
│                              ││bash-5.2$ █                                                       │
└──────────────────────────────┘└──────────────────────────────────────────────────────────────────┘
 FOCUS | 27x66 | Ctrl-\: normal mode
--- end screen ---
exit=0
```
Bar changed 37x86 -> 27x66 and `tput cols` went 86 -> 66 after resize. Pacer 60fps/sync-output deferral: NOT PROVEN (internal — covered by tests).

## e2e: RED shown (and in red)
Status: PROVEN
```console
$ python3 pty-harness (40x120) ... printf "\e[31mRED\e[0m\n" ; filter output for the RED cell
b'\x1b[38;5;1;49mRED'
```
Text `RED` visible in the dump above; the host-terminal bytes baton emits for it carry foreground colour 1 (red).

## e2e with fake-claude: `DA1: ok`
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:DA1: ok' --step dump --step 'send:\x1c' --step 'wait:NORMAL' --step 'send:q' --step 'expect-exit:0' -- baton spike -- fake-claude
│                              ││DA1: ok                                                                               │
│                              ││FAKE CLAUDE session=fde117b9-10b4-4f0d-b8e8-268d9ecf9470 model=claude-opus-5-5        │
│                              ││> █                                                                                   │
 FOCUS | 37x86 | Ctrl-\: normal mode
exit=0
```
fake-claude printed `DA1: ok` only after receiving a reply written back to the PTY.

## Default command is `claude` (optional real run, no prompt sent)
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:(trust|❯)' --step dump --step 'send:\x1c' --step 'wait:NORMAL' --step 'send:q' --step 'expect-exit:0' -- baton spike
│ ││ Quick safety check: Is this a project you created or one you trust? ...
│ ││ ❯ No, exit
│ ││   Yes, I trust this folder
 FOCUS | 37x86 | Ctrl-\: normal mode
exit=0
```
Real claude trust dialog rendered inside the panel; quit with exit 0. No prompt sent.

## e2e_spike.rs test
Status: PROVEN
```console
$ cargo test -p baton --test e2e_spike
test spike_answers_queries_for_fake_claude ... ok
test spike_embeds_bash_resizes_and_quits ... ok
test result: ok. 2 passed; 0 failed
```
(Its `resize:30x100` expects `27x66`; the plan text says 66 cols for tput and matches.)

## Host terminal setup (keyboard enhancement push, restore on panic)
Status: NOT PROVEN (internal — covered by tests); driver cannot observe a real host terminal's enhancement support or a panic.

## Manual checklist docs/m0-spike.md
Status: NOT PROVEN (manual, human-run against real claude; contents not evaluated here).
