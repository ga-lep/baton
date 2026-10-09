# M0 embedding spike: go/no-go for vt100

Run on 2026-10-09 against Claude Code 2.1.295 (`--model haiku`), driven headlessly
through `baton-drive` running `baton spike` (host terminal: 40x120, a `vt100`
emulation inside `baton-drive`, so nothing here was seen on a real terminal emulator).
Scratch directory: a throwaway dir under the session scratchpad (trust dialog accepted
once for that dir only). Three prompts were sent in total (permission prompt, a failed
permission attempt, an 80-line listing), all with haiku.

Reproduce (no prompt, no token cost):

```
D=$(mktemp -d)
target/debug/baton-drive --size 40x120 --step 'wait:trust this folder' --step dump \
  -- env -C "$D" target/debug/baton spike -- ~/.local/bin/claude --model haiku
```

`baton-drive` starts its child in `$HOME`, so wrap the command in `env -C <dir>`.
Keys sent immediately after a dialog appears can be lost: sleep ~2 s first.

## Checklist

| Item | Result | Observation |
|---|---|---|
| Trust dialog, startup | PASS | Dialog and Claude banner render correctly; Down + Enter select "Yes, I trust". Panel size shown as `37x86`. |
| Typing | PASS | Plain and non-ASCII text (`hello wörld`) echoed in the prompt. |
| Slash-command menu | PASS | `/` opens the command menu above the prompt; Esc closes it. |
| Permission prompt | PASS | With `--permission-mode default`, "Run bash: touch zz.txt" produced the "Do you want to proceed?" dialog with 4 options; Esc denied it ("Interrupted"). An allow-listed `echo` ran without any dialog (user settings), so that attempt did not show the dialog. |
| Shift-Enter newline | PASS | Through the spike, host `CSI 13;2u` became a newline. The encoder chose modifyOtherKeys (`ESC [ 27 ; 2 ; 13 ~`) because Claude enables `CSI >4;2m`. Tested directly against Claude, **all four** encodings inserted a newline: `ESC[27;2;13~` (modifyOtherKeys), `ESC[13;2u` (kitty), `ESC CR` (Meta-Enter) and `LF`. Keep modifyOtherKeys first. |
| Multi-line paste | PASS | Bracketed paste (`CSI 200~ ... CSI 201~`) arrived as three lines in the prompt, not submitted. |
| Resize | PASS | `resize:30x100` changed the bar to `27x66`; Claude reflowed (path shortened, border narrowed); restoring 40x120 returned to `37x86`. |
| Colors | PARTIAL | Cell colors are covered by the unit test `render_red_line`. The `baton-drive` dump is plain text, so colors of Claude's UI were not inspected. |
| Emoji / CJK / wide chars | PASS | `🎉 ✓ 日本語 é` rendered; the row was exactly 120 cells wide (borders aligned), so double-width cells are handled. |
| Fullscreen mouse scroll | PASS | Claude ran in its fullscreen TUI and enabled SGR mouse. SGR wheel events forwarded by the spike scrolled Claude's own view up (lines 52 to 49, "Jump to bottom" hint) and back down. |
| Flicker | NOT VERIFIED | Cannot be judged headlessly. The pacer defers rendering while synchronized output (`?2026`) is active, for at most 50 ms; needs a human look on a real terminal. |
| Quit | PASS | Ctrl-\ showed `NORMAL`; `q` killed the real Claude and `baton spike` exited 0. |

Other observations: the replies to DA1/DSR are produced by the spike itself (Claude
started without stalling), and no kitty query reply is sent, so Claude used
modifyOtherKeys as designed.

## Decision

**GO for vt100.** Every item that can be exercised headlessly worked against the real
Claude, including the fullscreen TUI, bracketed paste, wide characters, resize reflow
and mouse forwarding, with no emulation defects seen. The only open item is flicker,
which has a mitigation in place (sync-output deferral) and can be re-checked by a human
without any code change. If flicker or a rendering defect shows up there, insert the
task "AlacrittyScreen impl of Screen" before Task 7; the `Screen` trait keeps that swap local.
