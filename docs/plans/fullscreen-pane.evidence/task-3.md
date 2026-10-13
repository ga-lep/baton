# Evidence — Task 3: borderless fullscreen rendering, bar and spec text
Commit: 7637921
Environment: debug build (`cargo build --workspace --bins`); real daemon + `baton` TUI driven by `target/debug/baton-drive --size 40x120`, session = `fake-claude`. Scratch config/state in a dir outside the repo (runtime dir /tmp/bt3run, removed afterwards; daemon stopped). Env: BATON_CONFIG/STATE_DIR/RUNTIME_DIR, BATON_NOTIFY_SINK=off, BATON_NO_UPDATE_CHECK=1. Dumps are trimmed with `sed`/`grep` where noted (middle blank rows removed).

## 120x40 fullscreen: no sidebar/borders/titles, row 0 is the mirror's first row, row 39 is the bar
Status: PROVEN

```console
$ baton-drive --size 40x120 --step 'wait:FAKE CLAUDE session=' --step 'sleep:300' --step dump --step 'send:z' --step 'sleep:300' --step dump --step 'send:z' --step 'sleep:300' --step dump -- baton
--- screen 40x120 ---          (dump 1, before z: normal layout, trimmed to first 3 and last 2 rows)
┌Projects──────────────────────┐┌r · p · ○ idle──────────...──┐
│▾ x                           ││DA1: ok                      ...
│  1 ○ r  idle                 ││FAKE CLAUDE session=998c58f7-87e2-47b7-956f-3decb02aa7ee model=claude-opus-5-5 ...
...
┌Session───────────────────────┐│
...
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit
--- screen 40x120 ---          (dump 2, after z, rows 0-3 and 39; rows 4-38 are empty)
DA1: ok
FAKE CLAUDE session=998c58f7-87e2-47b7-956f-3decb02aa7ee model=claude-opus-5-5
>
(35 blank rows)
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit
--- screen 40x120 ---          (dump 3, after second z: normal layout again)
┌Projects──────────────────────┐┌r · p · ○ idle──...
│▾ x                           ││DA1: ok
│  1 ○ r  idle                 ││FAKE CLAUDE session=998c58f7-... model=claude-opus-5-5
...
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit
```
Fullscreen dump has no `Projects`, no `Session` title, no box-drawing characters; the mirror's first row (`DA1: ok`) is at column 0 of row 0, `FAKE CLAUDE session=` at column 0 of row 1, and row 39 is ` NORMAL │ ... z fullscreen ...`. Second `z` restores the normal layout (also proves "not-fullscreen rendering" is intact: dump 1 and 3 show sidebar, session panel, borders, title and the bar with `z fullscreen` before `? help`).

(Note: the fake-claude mirror's first row is `DA1: ok`, the fake's own output; the `FAKE CLAUDE session=` line is row 1.)

## Focus mode bar: ` FOCUS │ ` and `Alt-z fullscreen`
Status: PROVEN

```console
$ baton-drive ... --step 'send:\r' --step 'sleep:200' --step 'send:\ez' --step 'sleep:300' --step dump -- baton   (rows 0-3 and 39 shown)
--- screen 40x120 ---
DA1: ok
FAKE CLAUDE session=998c58f7-87e2-47b7-956f-3decb02aa7ee model=claude-opus-5-5
> █

 FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session  Alt-z fullscreen
--- end screen ---
```
Enter focused, Alt-z went fullscreen (no borders), bar is FOCUS with `Alt-z fullscreen` at the end.

## Pending restart confirmation renders in row 39 as ` NORMAL │ <prompt>`; editor error flash renders there too, view stays borderless
Status: PROVEN

```console
$ baton-drive ... --step 'send:\r' --step 'send:perm\r' --step 'send:\x1c' --step 'send:z' --step 'send:r' --step dump -- baton   (rows 0-5, 39)
--- screen 40x120 ---
DA1: ok
FAKE CLAUDE session=998c58f7-87e2-47b7-956f-3decb02aa7ee model=claude-opus-5-5
> perm
permission needed
>

 NORMAL │ Restart r? [y/N]

$ (config with editor = "definitely-not-an-editor-xyz {path}") baton-drive ... --step 'send:z' --step 'send:e' --step dump -- baton   (rows 0-3, 39)
DA1: ok
FAKE CLAUDE session=998c58f7-87e2-47b7-956f-3decb02aa7ee model=claude-opus-5-5
> perm

 NORMAL │ cannot run definitely-not-an-editor-xyz: No such file or directory (os error 2)
```
Both messages replace the bar text in row 39 while the mirror is still borderless. (A session in the quiet/idle state restarts without a prompt by design, so the fake was put in a permission state first.)

## Overlays still render in fullscreen (help); help lists `fullscreen` in both columns; remapped key appears
Status: PROVEN

```console
$ baton-drive ... --step 'send:z' --step 'send:?' --step dump   (grep for relevant rows; box trimmed)
          ┌Key bindings (? or Esc to close)─────────────────────────
          │activate        enter, l                         fullscreen      alt-z                            │
          │fullscreen      z, alt-z                                                                          │
          │help            ?                                                                                 │
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit

$ (config [keybindings.normal] fullscreen = "f") baton-drive ... --step 'send:?' --step dump | grep fullscreen
│         │activate        enter, l                         fullscreen      alt-z                            │         │
│tokens   │fullscreen      f                                                                                 │         │
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  f fullscreen  ? help  q quit

$ (same remap) ... --step 'send:f' --step dump   (rows 0-2, 39)
DA1: ok
FAKE CLAUDE session=998c58f7-... model=claude-opus-5-5

 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  f fullscreen  ? help  q quit
```
Help overlay is drawn over the fullscreen mirror; `fullscreen` appears in the Focus column (`alt-z`) and Normal column (`z, alt-z`); with remap the normal row shows `f`, the bar shows `f fullscreen`, and `f` toggles fullscreen.

## `Esc` closes help before it would exit fullscreen
Status: PROVEN

```console
$ baton-drive ... --step 'send:z' --step 'send:?' --step dump --step 'send:\e' --step dump --step 'send:\e' --step dump   (non-blank rows)
dump 1: help box over fullscreen mirror
dump 2 (first Esc): DA1: ok / FAKE CLAUDE session=... / > / bar ` NORMAL │ ... z fullscreen ...`   (help gone, still borderless)
dump 3 (second Esc): ┌Projects──...  sidebar, Session panel, borders back
```
First Esc closed help and stayed fullscreen; second Esc left fullscreen.

## `normal_bar` / `focus_bar` text and `docs/SPEC.md` match
Status: PROVEN

```console
$ grep -n "Alt-z\|z fullscreen" docs/SPEC.md
91: NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit
95:`FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session  Alt-z fullscreen`.
125:| `z` / `Alt-z` | Toggle fullscreen: ...
136:| `Alt-z` | Toggle fullscreen (stays in focus mode) |
```
SPEC lines 91/95 are identical to the bars rendered above (`z fullscreen` before `? help`; `Alt-z fullscreen` last).

## Supporting unit tests (not a substitute)
```console
$ cargo test -p baton fullscreen
test tui::ui::tests::help_overlay_renders_in_fullscreen ... ok
test tui::ui::tests::help_shows_a_remapped_fullscreen_key ... ok
test tui::ui::tests::fullscreen_draws_only_the_mirror_and_the_bar ... ok
test tui::ui::tests::fullscreen_keeps_confirm_prompt_and_flash_in_the_bar ... ok
test result: ok. 13 passed; 0 failed
```

Cleanup: `baton daemon stop` run, /tmp/bt3run removed, `git status --short` clean apart from this file. (Pre-existing unrelated baton processes, pids 5867 and 10103, were not started by me and were left alone.)

## Verdict
EVIDENCE: PROVEN
