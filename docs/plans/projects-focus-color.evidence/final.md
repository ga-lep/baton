# Evidence — Projects pane focus colour (whole feature)
Commit: ee280d4
Environment: debug build (`cargo build --workspace --bins`), real `baton` TUI + daemon with `fake-claude` as the profile command, driven in a 120x40 PTY by a scratch Rust consumer (portable-pty 0.9 + vt100 0.16 + baton_core Scanner answering terminal queries, as baton-testkit's Drive does) kept outside the repo. Cell colour/bold read from the vt100 screen (`fgcolor()`, `bold()`); `Idx(3)` is ANSI yellow. Env: BATON_CONFIG/STATE_DIR/RUNTIME_DIR under /tmp/bfc (mode 0700), BATON_NOTIFY_SINK=off, BATON_NO_UPDATE_CHECK=1. Key flow: start; `o` (open session); Enter; Ctrl-\; `?`; Esc; `z`; Esc; Enter, Alt-z; Alt-z. Output below is trimmed: title letters 'r','o','j','e','c','t','s' (cols 2..8) are omitted from the listing (only 'P' at col 1 shown); in the actual run all 8 title cells matched 'P' in every phase (yellow+bold in normal mode, default otherwise).

## Unit tests: `cargo test -p baton tui::ui`
Status: PROVEN
```console
$ cargo test -p baton tui::ui   (filtered to the new tests + summary)
test tui::ui::tests::normal_mode_highlights_the_projects_pane ... ok
test tui::ui::tests::focus_mode_highlights_the_session_pane ... ok
test tui::ui::tests::update_notice_is_not_highlighted ... ok
test tui::ui::tests::help_overlay_keeps_the_projects_highlight ... ok
test tui::ui::tests::fullscreen_has_no_pane_highlight ... ok
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 179 filtered out
```
All four new tests plus the notice test pass.

## Startup, normal mode: Projects border and title yellow + bold, main block default
Status: PROVEN
```console
== startup (normal mode, no session opened)
  Projects corner            ( 0,  0) "┌" fg=Idx(3) bold=true
  Projects left edge         ( 1,  0) "│" fg=Idx(3) bold=true
  Projects title 'P'         ( 0,  1) "P" fg=Idx(3) bold=true
  Session info corner        (23,  0) "┌" fg=Default bold=false
  Main block corner          ( 0, 32) "┌" fg=Default bold=false
  Main block left edge       ( 1, 32) "│" fg=Default bold=false
  bottom row: " NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit"
```
Projects corner/edge/title are fg=Idx(3) bold=true; Session info and main block corners are Default, not bold. Identical once a session is open (`== normal mode, session open` phase, same values).

## After Enter on a session (focus): main block yellow + bold, Projects default
Status: PROVEN
```console
== focus mode (after Enter)
  Projects corner            ( 0,  0) "┌" fg=Default bold=false
  Projects left edge         ( 1,  0) "│" fg=Default bold=false
  Projects title 'P'         ( 0,  1) "P" fg=Default bold=false
  Session info corner        (23,  0) "┌" fg=Default bold=false
  Main block corner          ( 0, 32) "┌" fg=Idx(3) bold=true
  Main block left edge       ( 1, 32) "│" fg=Idx(3) bold=true
  bottom row: " FOCUS │ Ctrl-\\ back  Alt-n next-attention  Alt-1..9 session  Alt-z fullscreen"
```
Highlight moved: main block corner/edge yellow+bold, Projects corner/edge/title Default, bar reads FOCUS.

## After Ctrl-\ : back to Projects highlight
Status: PROVEN
```console
== back to normal (after Ctrl-\)
  Projects corner            ( 0,  0) "┌" fg=Idx(3) bold=true
  Projects left edge         ( 1,  0) "│" fg=Idx(3) bold=true
  Projects title 'P'         ( 0,  1) "P" fg=Idx(3) bold=true
  Session info corner        (23,  0) "┌" fg=Default bold=false
  Main block corner          ( 0, 32) "┌" fg=Default bold=false
  Main block left edge       ( 1, 32) "│" fg=Default bold=false
  bottom row: " NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit"
```
Projects yellow+bold again, main block default, bar reads NORMAL.

## `?` help open (normal mode): Projects highlight stays
Status: PROVEN
```console
== help overlay open (normal)
  Projects corner            ( 0,  0) "┌" fg=Idx(3) bold=true
  Projects left edge         ( 1,  0) "│" fg=Idx(3) bold=true
  Projects title 'P'         ( 0,  1) "P" fg=Idx(3) bold=true
  Session info corner        (23,  0) "┌" fg=Default bold=false
  Main block corner          ( 0, 32) "┌" fg=Default bold=false
  Main block left edge       ( 1, 32) "│" fg=Default bold=false
  bottom row: " NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit"
```
With the Key bindings overlay on screen (waited for the text), Projects remains yellow+bold; main block default. After Esc the same state held (`== help closed (Esc)`).

## After `z` fullscreen: no yellow above the bar
Status: PROVEN
```console
== fullscreen normal: yellow cells in rows 0..38: 0; bottom: " NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit"
== fullscreen focus: yellow cells in rows 0..38: 0; bottom: " FOCUS │ Ctrl-\\ back  Alt-n next-attention  Alt-1..9 session  Alt-z fullscreen"
```
Zero yellow cells in rows 0..38 in fullscreen normal and fullscreen focus (Enter then Alt-z); the bar still shows the mode. Esc from fullscreen restored the Projects highlight and Alt-z from fullscreen focus restored the main-block highlight (`after Esc leaving fullscreen`, `after Alt-z back (focus)` phases).

## Cleanup
`baton daemon stop` printed `daemon stopped` (exit 0); /tmp/bfc removed; scratch consumer lives in the session scratchpad. No repo files changed except this one. The user's own baton processes were not touched.
