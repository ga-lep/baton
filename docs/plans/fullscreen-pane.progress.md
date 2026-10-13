Task 1: started
Task 1: complete (c3ae668..48a7db5, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/fullscreen-pane.evidence/task-1.md
  notes: security info — focus-mode alt-z no longer reaches the PTY, mention in docs (Task 5); review suggestions — assert KeymapError::Conflict variant rather than message text; split the combined keymap test
Task 2: started
Task 2: complete (b6cdc7d..4bada7e, gate ok, security PASS, review PASS, evidence N/A)
  evidence: docs/plans/fullscreen-pane.evidence/task-2.md
  notes: security Low — fullscreen can outlive its session if it is killed/removed while connected (empty pane until z/Esc); review — remove #[allow(dead_code)] on is_fullscreen/wants_mouse_capture once used; consider dedicated exit_fullscreen for Esc path
Task 3: started
Task 3: complete (4848c16..7637921, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/fullscreen-pane.evidence/task-3.md
  notes: security Low — no session identity visible in fullscreen (session output could mimic baton UI); suggest showing session name in the bar. Review — draw_chrome guard has no red test (layout already zero-sized); no explicit test that Esc closes help before exiting fullscreen; bar match calls confirm_prompt()/flash() twice (pre-existing)
Task 4: started
Task 4: complete (56e565c..3d3f842, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/fullscreen-pane.evidence/task-4.md
  notes: real mouse-drag selection NOT PROVEN (manual check for user). Security Low — 1007h forced on at exit regardless of host's original state (DECRQM query would fix); set ALT_SCROLL_RESET before writing 1007l; SIGTERM/SIGHUP skip restore (pre-existing). Review — restore() calling restore_alt_scroll untested; global ALT_SCROLL_RESET shared by tests; e2e "hi" wait could match pre-existing text
Task 5: started
