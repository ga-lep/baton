Task 1: started
Task 1: complete (af825c9..dffe86d, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-1.md
  notes: [sec Low] hook ignores stdin / free-form event (handle in Task 13: capped stdin read, closed event enum); [sec Low] spike must exec argv without shell, debug ungated; [review Warn] `baton hook` with bad args exits 2 via clap — make hook path robust (Task 13); [review Warn] no runtime exit-code tests
Task 2: started
Task 2: complete (9aa50a6..1b7386a, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-2.md
  notes: [sec Low] scanner stays in OSC/DCS string on ESC+non-\ (diverges from vt100/ECMA-48; could leave sync_output stale) and ignores 8-bit C1 — fix when wiring Screen (Task 5); [sec Info] query replies unthrottled — consider rate limit in daemon; [review] kitty_pop(0) no-op, colon sub-params unparsed
