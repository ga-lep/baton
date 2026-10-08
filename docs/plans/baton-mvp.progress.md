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
Task 7: complete (b47245e..40a19d5, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-7.md
  notes: [sec Low] for daemon tasks: socket dir 0700 + sock 0600 + SO_PEERCRED uid check, Hello.role not an authz signal; validate Resize/Attach rows/cols (0 or >1000) and clamp GetScrollback count; [sec Low] SessionId::from_repo lossy on non-UTF-8 paths, '/' in project names ambiguous; [sec Info] atomic-polyfill unmaintained via postcard->heapless; [review] encode returns Result (deliberate), trailing bytes reported as SerdeDeCustom
Task 8: started
Task 8: complete (26020e3..e907070, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-8.md
  notes: round 1 security FAIL [High] /tmp/baton-<uid> runtime dir trusted without owner/mode check -> fixed in e907070 (ensure_private_dir: no symlink, uid match, mode&077==0; HOME falls back to passwd); [sec Low] socket_path()/hooks_json_path() don't themselves enforce ensure_runtime_dir — daemon must call ensure_runtime_dir() first; [sec Low] editor template: split with shell_words THEN substitute {path} per-arg, never sh -c (Task 18); [sec Info] no perms check on config file; toml errors may echo a line; [review] deny_unknown_fields strict
Task 9: started
Task 9: complete (a4757f1..a322672, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-9.md
  notes: [sec MEDIUM] client connect() trusts whatever listens on socket path: no runtime-dir check, no server peer uid check; start() connects before ensure_runtime_dir — carried into Task 10; [sec Low] daemon.log perms depend on umask, state dir not checked; [sec Low] killpg on registered pgid without pgid<=1 / own-group / reaped checks; [sec Info] no idle timeout after Hello, 64x16MiB buffers; [review] stop prints extra 'daemon stopped'; SIGTERM path lacks e2e test; start racing a shutting-down daemon gives generic timeout
Task 10: started
Task 10: complete (4f199d4..c82d1cb, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-10.md
  notes: carried-over fixes landed: client runtime-dir + server peer-uid check (Task 9 Medium), pgid guards + unregister on reap, size validation 1..=1000, scrollback clamp 10k, explicit repo cwd; [sec Low] spawn() leaks child if reader/writer setup fails after spawn; [sec Low] large scrollback/snapshot can exceed 16MiB frame -> silent disconnect; MAX_DIM 1000 x 10k scrollback memory; [sec Low] Input payload uncapped (cap ~64KiB); [sec Low] debug scrollback passes C1/BEL to terminal; [review] slow client dropped silently (client=None but conn open); open_project holds registry mutex during spawn
Task 11: started
