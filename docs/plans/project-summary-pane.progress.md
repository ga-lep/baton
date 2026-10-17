Task 1: started
Task 1: complete (ce674eb..4cc4aa8, gate ok, security PASS, review PASS, evidence N/A)
  evidence: docs/plans/project-summary-pane.evidence/task-1.md
  notes: clip counts chars not display width (pre-existing); activity() has redundant needs_attention check; Task 2 must remove temporary #[allow(dead_code)] (mod.rs, two App methods); package name is baton-tui
Task 2: started
Task 2: complete (b9667bc..a1e2f3f, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/project-summary-pane.evidence/task-2.md
  notes: WARNING (non-blocking): ui test project_summary_shows_the_branch is weak (main panel title also has "⎇ main · "), make sidebar-specific; loose !contains("Session") check; clip counts chars not width; draw_chrome entry-building could be a helper
Task 3: started
Task 3: complete (91e9d82..db66caa, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/project-summary-pane.evidence/task-3.md
  notes: Task 2 weak-branch-test warning fixed here; e2e uses fixed sleep polling (possible CI flake); loose "  p" and "Session" waits
