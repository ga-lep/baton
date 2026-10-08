Task 1: started
Task 1: complete (af825c9..dffe86d, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-1.md
  notes: [sec Low] hook ignores stdin / free-form event (handle in Task 13: capped stdin read, closed event enum); [sec Low] spike must exec argv without shell, debug ungated; [review Warn] `baton hook` with bad args exits 2 via clap — make hook path robust (Task 13); [review Warn] no runtime exit-code tests
