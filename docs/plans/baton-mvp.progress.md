Task 1: started
Task 1: complete (af825c9..dffe86d, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-1.md
  notes: [sec Low] hook ignores stdin / free-form event (handle in Task 13: capped stdin read, closed event enum); [sec Low] spike must exec argv without shell, debug ungated; [review Warn] `baton hook` with bad args exits 2 via clap — make hook path robust (Task 13); [review Warn] no runtime exit-code tests
Task 2: started
Task 2: complete (9aa50a6..1b7386a, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-2.md
  notes: [sec Low] scanner stays in OSC/DCS string on ESC+non-\ (diverges from vt100/ECMA-48; could leave sync_output stale) and ignores 8-bit C1 — fix when wiring Screen (Task 5); [sec Info] query replies unthrottled — consider rate limit in daemon; [review] kitty_pop(0) no-op, colon sub-params unparsed
Task 3: started
Task 3: complete (bb6f558..88b4f84, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-3.md
  notes: [evidence] baton-drive child cwd defaults to $HOME (portable-pty CommandBuilder default) — daemon/spike MUST set cwd explicitly; [sec Low] fake-claude accepts non-UUID session ids (path traversal in test tool, real claude rejects) ; [sec Low] unquoted temp path in drive_smoke hook cmd; [sec Info] Drive Drop kills without wait (zombies); [review] malformed settings silently ignored in fake-claude
Task 4: started
Task 4: complete (b5fc412..ba704a4, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-4.md
  notes: [sec Low] encode_paste strip loop is O(n^2) on nested markers — prefer single-pass ESC filter; [sec Info] C1 CSI (U+009B) end marker not stripped; [sec/review] encode_mouse doesn't bound right/bottom edge despite doc; [review] Shift-Enter prefers modifyOtherKeys over kitty — confirm in Task 6 spike
Task 5: started
Task 5: complete (d6a33ca..3b1c604, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-5.md
  notes: [sec MEDIUM] Vt100Screen new/resize with 0 rows/cols panics (debug) or wraps (release) in vt100 Grid::set_size; scrollback_rows loops forever when rows==0 — clamp .max(1) (carry into Task 6); [sec Low] window title not sanitized/capped (strip controls, cap 256); [sec Info] document snapshot() not safe to write to host terminal; [review] snapshot/scrollback take &mut self; view offset restore untested
Task 6: started
Task 6: complete (41b6ab2..66570ab, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-6.md
  notes: M0 GO for vt100 (flicker + full color check need a human on a real terminal); real-claude runs added a trust entry for a scratch dir in ~/.claude-personal; Shift-Enter: all 4 encodings accepted, modifyOtherKeys kept first; [sec Low] child inherits full env + PATH lookup of claude; [sec/review] panic hook chained on every TerminalGuard::enter (use Once); [review] wheel/focus forwarded in normal mode too
Task 7: started
