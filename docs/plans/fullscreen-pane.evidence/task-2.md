# Evidence — Task 2: fullscreen state, layout and resize in the reducer
Commit: 4bada7e
Environment: no external surface (reducer state only); targeted tests run by name with `cargo test -p baton`.

```console
$ cargo test -p baton fullscreen   (filtered to matching lines)
test spike::tests::fullscreen_layout_covers_all_but_the_bar ... ok
test tui::app::tests::fullscreen_without_a_session_sets_a_hint ... ok
test tui::app::tests::esc_leaves_fullscreen_in_normal_mode_only_when_fullscreen ... ok
test tui::app::tests::fit_keeps_the_fullscreen_layout ... ok
test tui::app::tests::restart_prompt_and_editor_error_keep_fullscreen ... ok
test tui::app::tests::z_toggles_fullscreen_in_normal_mode_and_resizes ... ok
test tui::app::tests::alt_z_toggles_fullscreen_in_focus_mode_without_forwarding ... ok
test tui::app::tests::fullscreen_mouse_origin_is_the_top_left_corner ... ok
test tui::app::tests::connection_changes_clear_fullscreen ... ok
test result: ok. 9 passed; 0 failed
```

## All criteria (layout, state, toggle/resize, focus/normal Esc, no-session hint, restart/editor-error, disconnect/mismatch/connect clearing, mouse origin, wants_mouse_capture)
Status: NOT PROVEN (internal — covered by tests); the named tests above all pass and map to each criterion. wants_mouse_capture is asserted inside z_toggles_fullscreen_in_normal_mode_and_resizes (tests.rs:1122,1125). No user-visible surface exists until Tasks 3/4.
