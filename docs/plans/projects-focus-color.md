# Projects Pane Focus Color Plan
Status: APPROVED

## Goal
Make it visible which pane receives keys in the baton TUI. Today only focus mode is marked: the main (session) panel border turns yellow and bold (`crates/baton/src/tui/ui.rs::draw_chrome`), and in normal mode nothing is highlighted. After this change, the Projects block (the sidebar list) has a yellow, bold border and title in normal mode, when keys act on the sidebar. In focus mode the highlight moves to the main panel border, as it does today, and the Projects block goes back to the default style. This uses the TUI's existing palette and its "active pane = Yellow + BOLD border" convention.

## Findings (read from the repo on 2026-10-13)
1. **Drawing.**
   - `ui.rs::draw` computes `focus = app.mode == Mode::Focus`. It draws the chrome only when `!app.is_fullscreen()`, then the mirror, then the bottom bar (`Modifier::REVERSED`, with a ` NORMAL │ ` / ` FOCUS │ ` prefix), then the overlays.
   - `draw_chrome` builds the `Projects` block with `Block::default().borders(Borders::ALL).title("Projects")`, plus an optional `title_bottom` update notice. Under it sits the `Session` info block, then the main block. The main block's `border_style` is `fg(Yellow) + BOLD` in focus mode and `Style::default()` otherwise.
2. **Palette.**
   - Yellow + BOLD means the focused pane (main border) and attention (`sidebar.rs::Row::style`, for attention sessions and collapsed projects that hide one).
   - Red is used for errors (config error popup border, info panel bars at 90% or more).
   - DIM is used for closed projects and the config popup hint; DarkGray for `n/a` in the info panel.
   - Attention is shown on row text, never on borders, so a yellow border cannot be confused with it.
3. **Ratatui 0.30.** `Block::border_style` styles only the border cells, not the title. To color the title, use a styled `Line` (`Line::styled("Projects", style)`), which leaves the bottom "vN available" notice unstyled.
4. **Fullscreen.** `draw_chrome` is skipped entirely, so no pane border is drawn and the change has no effect there. The bar's mode name still shows the mode.
5. **Overlays.** Help, disconnected, version mismatch and config error are drawn after the chrome, centered, so they cover only part of the sidebar. Connection loss and mismatch set `mode = Normal`. A confirm prompt keeps normal mode.
6. **Docs.** `docs/SPEC.md` line 94 says "In focus mode, the main panel border is highlighted"; it should also say what normal mode highlights.
7. **Tests.** The `ui.rs` tests render into `TestBackend::new(120, 40)` through `render()` and helpers such as `app_with_session()`, `press()` and `fullscreen_app()`. With that layout the Projects block starts at (0,0), and the main block starts at `app.layout.main`.

## Decisions
- **One shared style.** Add `fn active_border() -> Style` (Yellow + BOLD) in `ui.rs`. The main panel border in focus mode and the Projects border and title in normal mode both use it, so the two cannot drift apart.
- **Only the Projects block is highlighted, not the `Session` info block.** Keys move the list cursor; the info panel is read-only.
- **Inactive panes keep `Style::default()`.** There is no extra dimming: the main border in normal mode looks as it does today. A yellow border next to a default one is enough contrast, and it avoids gray-on-gray problems on light themes.
- **The main block title stays unstyled.** It carries session info and the status badge, and its look in focus mode does not change. Only the Projects title is styled, because it is the pane's name.
- **Overlays do not change the highlight.** It follows `app.mode` only. Under an overlay the sidebar keeps its normal-mode highlight, and the overlay itself takes the keys. This keeps the logic to a single condition.
- **Fullscreen does nothing extra.** No chrome is drawn there.

### Task 1: Highlight the Projects pane in normal mode
Language: rust
Files: crates/baton/src/tui/ui.rs, docs/SPEC.md
Depends on: none
Acceptance criteria:
- `ui.rs` has a single `active_border()` style (`fg(Color::Yellow)` + `Modifier::BOLD`). It is used for the main block's `border_style` in focus mode and for the Projects block's `border_style` and title in normal mode.
- New test `normal_mode_highlights_the_projects_pane`: after rendering `app_with_session()` (normal mode) into a `TestBackend`, these cells have fg `Yellow` and the `BOLD` modifier:
  - the Projects top-left corner `(0,0)`;
  - a left-border cell `(0,1)`;
  - every cell of the `Projects` title text on row 0.
- In the same test, the main block's top-left corner (`app.layout.main.x`, `app.layout.main.y`) has fg `Reset` with no `BOLD`, and the `Session` info block's top-left corner is not yellow.
- New test `focus_mode_highlights_the_session_pane`: with `app.mode = Mode::Focus`, the main block corner is Yellow + BOLD, and the Projects corner `(0,0)` and its title cells are not yellow.
- New test `fullscreen_has_no_pane_highlight`: on `fullscreen_app()`, no cell in rows 0..39 has fg `Yellow`, in both normal and focus mode.
- New test `help_overlay_keeps_the_projects_highlight`: with `Overlay::Help` open in normal mode, the Projects left border at `(0,1)` is still Yellow + BOLD.
- With `update_available` set, the bottom notice ` vX available ` cells are not yellow.
- In `docs/SPEC.md`, the sentence near line 94 says that in normal mode the Projects pane border and title are highlighted, and in focus mode the main panel border is highlighted.
- `cargo test -p baton`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass.
Evidence: `cargo test -p baton tui::ui` passes the four new tests. In a real terminal, running `baton` in normal mode shows a yellow, bold Projects border and title. Pressing `⏎` on a session moves the yellow border to the session panel, and `Ctrl-\` moves it back.
