# Evidence — Task 1: highlight the Projects pane in normal mode
Commit: 979099a
Environment: debug build of `baton` with `fake-claude` as the session command, driven in a 120x40 PTY by a scratch Rust consumer (portable-pty + vt100 0.16 + baton_core Scanner, as baton-testkit's Drive does) kept outside the repo. Cell colour/bold were read from the vt100 screen (`fgcolor()`, `bold()`); `Idx(3)` is ANSI yellow. Env: BATON_CONFIG/STATE_DIR/RUNTIME_DIR under a short /tmp dir, BATON_NOTIFY_SINK=off, BATON_NO_UPDATE_CHECK=1. Harness daemon and fake-claude processes were stopped (`baton daemon stop`), temp dir removed. The user's own baton processes were not touched.

## Unit tests (evidence hint)
Status: PROVEN
```console
$ cargo test -p baton tui::ui   (filtered)
test tui::ui::tests::normal_mode_highlights_the_projects_pane ... ok
test tui::ui::tests::focus_mode_highlights_the_session_pane ... ok
test tui::ui::tests::fullscreen_has_no_pane_highlight ... ok
test tui::ui::tests::help_overlay_keeps_the_projects_highlight ... ok
test tui::ui::tests::update_notice_is_not_highlighted ... ok
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 179 filtered out
```

## Normal mode: Projects border and `Projects` title Yellow + BOLD; main block corner default; Session info block not yellow
Status: PROVEN
```console
== normal mode (before Enter)
  Projects corner                    ( 0,  0) "┌" fg=Idx(3) bold=true
  Projects left edge                 ( 1,  0) "│" fg=Idx(3) bold=true
  Projects title 'P'                 ( 0,  1) "P" fg=Idx(3) bold=true
  Projects title 's' (last)          ( 0,  8) "s" fg=Idx(3) bold=true
  Session info corner                (23,  0) "┌" fg=Default bold=false
  Main block corner                  ( 0, 32) "┌" fg=Default bold=false
  Main block left edge               ( 1, 32) "│" fg=Default bold=false
  bottom row: " NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  z fullscreen  ? help  q quit"
```
Real binary: Projects border/title are yellow+bold; main block and Session info are default.

## Focus mode: main block border Yellow + BOLD; Projects border/title not yellow
Status: PROVEN
```console
== focus mode (after Enter)
  Projects corner                    ( 0,  0) "┌" fg=Default bold=false
  Projects left edge                 ( 1,  0) "│" fg=Default bold=false
  Projects title 'P'                 ( 0,  1) "P" fg=Default bold=false
  Projects title 's' (last)          ( 0,  8) "s" fg=Default bold=false
  Session info corner                (23,  0) "┌" fg=Default bold=false
  Main block corner                  ( 0, 32) "┌" fg=Idx(3) bold=true
  Main block left edge               ( 1, 32) "│" fg=Idx(3) bold=true
  bottom row: " FOCUS │ Ctrl-\\ back  Alt-n next-attention  Alt-1..9 session  Alt-z fullscreen"
== back to normal (after Ctrl-\)
  Projects corner                    ( 0,  0) "┌" fg=Idx(3) bold=true
  ...                                (identical to the "before Enter" block: Projects yellow+bold, main block default)
```
Enter moves the highlight to the session panel; Ctrl-\ moves it back (the full "back to normal" output matched the first phase line for line).

## Fullscreen: no yellow in rows above the bar, in both modes
Status: PROVEN
```console
== fullscreen normal: cells with fg=Idx(3)(yellow) in rows 0..39: 0; bottom: " NORMAL │ ⏎ focus  n next-attention ..."
== fullscreen focus: yellow cells rows 0..39: 0; bottom: " FOCUS │ Ctrl-\\ back  Alt-n next-attention  Alt-1..9 session  Alt-z fullscreen"
```
Zero yellow cells in rows 0..38 in both modes (unit test fullscreen_has_no_pane_highlight also passes).

## Help overlay open in normal mode: Projects border stays Yellow + BOLD
Status: PROVEN
```console
== help overlay open (normal)   (after `?`, "Key bindings" on screen)
  Projects corner                    ( 0,  0) "┌" fg=Idx(3) bold=true
  Projects left edge                 ( 1,  0) "│" fg=Idx(3) bold=true
  Projects title 'P'                 ( 0,  1) "P" fg=Idx(3) bold=true
  Projects title 's' (last)          ( 0,  8) "s" fg=Idx(3) bold=true
  Main block corner                  ( 0, 32) "┌" fg=Default bold=false
```

## With an update available, the ` vX available ` notice is not yellow
Status: NOT PROVEN in the real binary (needs a fake release server; BATON_NO_UPDATE_CHECK=1 was used). Covered by the passing unit test `update_notice_is_not_highlighted` (all 18 notice cells checked non-yellow in a TestBackend render with the Projects border active). The diff applies `Style::reset()` to the notice line.

## docs/SPEC.md describes this
Status: PROVEN
```console
$ git diff 358078582b11..979099a -- docs/SPEC.md
+In normal mode, the Projects pane border and title are highlighted; in focus
+mode, the main panel border is highlighted and the bottom bar reads
```
