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
Task 11: complete (a1ddf75..bdede1c, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-11.md
  notes: M1 reached (TUI attach/focus/quit/reattach). carried-over fixes landed: Once panic hook, focus/wheel only in focus mode, Input cap 64KiB + TUI chunking, slow client kicked with Error+close; [sec Low] restart_daemon SIGTERMs SO_PEERCRED pid unchecked (pid 0 -> own pgrp; not verified to be baton) — filter pid>0, verify /proc/pid/exe; [sec Low] Output flood can kick client repeatedly (queue by frames not bytes); [sec Low] split bracketed paste failing mid-way leaves paste mode open; [review] y-restart path has no e2e test
Task 12: started
Task 12: complete (993422b..2e94676, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-12.md
  notes: M2 reached. carried-over fixes landed: restart_daemon pid>0 + /proc/pid/exe match, open_project no longer holds registry lock across spawn (open_gate); server suppresses OpenProject reply for attached conns (evidence: opening 2nd project keeps 1st's sessions); implementer skipped strict red-first on e2e; [review Warn] no direct test for reply suppression/open_gate; scrollback refetches whole history per scroll session; Loading state stuck if GetScrollback errors; [sec Low] pid reuse window in restart_daemon (pidfd would fix); ' (deleted)' suffix match is textual
Task 13: started
Task 13: complete (c12da7f..ada5c45, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-13.md
  notes: hook contract: hooks.json (9 events, 0600, atomic, POSIX-quoted exe) passed via --settings after profile argv; baton hook silent+exit 0 in all cases; PROTOCOL_VERSION->2 (SessionInfo.transcript_path); test bash profiles use '-s --' since --settings always injected (wrapper profiles like 'sh -c'/'ssh' would swallow the flag); real-claude SessionStart not exercised; [sec Low] SessionStart session_id/transcript_path/model stored unvalidated/unbounded — validate (len, no control chars, absolute path, under projects dir) before Task 17 tails transcript; [sec Low] connect_to doc overstates check for BATON_SOCK override; [sec Info] log session id with {:?}; [review] CONNECT_DEADLINE 250ms vs spec 200ms
Task 14: started
Task 14: complete (24ed80c..363280e, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-14.md
  notes: M3a status/attention. carried-over: SessionStart field validation (len caps, no control chars, absolute path) + {:?} id logging; baton-core now depends on baton-proto; terminal_focused sent but unused (for Task 15); [sec Low] hook event name logged unescaped ({event:?}); hook cmds on unbounded channel — cap event len / bounded try_send; [sec NOTE for Task 17] transcript_path accepts any absolute path — canonicalize and require under <profile config dir>/projects before reading
Task 15: started
Task 15: complete (453e01a..e20d22b, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-15.md
  notes: M3 reached (notifications via log sink only; real D-Bus not exercised, user asleep). carried-over: hook event name validation (1..64 ASCII alnum) + {:?} logging; all daemon-starting tests set BATON_NOTIFY_SINK=off; [evidence] long BATON_RUNTIME_DIR -> 'path must be shorter than SUN_LEN' on bind — consider a clear error message; [sec Low] notification text: escape <>& for markup servers, drop Unicode Cf (bidi/zero-width); [sec Low] LogSink falls back to ./ when state_dir fails, no O_NOFOLLOW; [review] unknown BATON_NOTIFY_SINK value (typo) silently means real D-Bus — warn
Task 16: started
Task 16: complete (19545e8..6a3c8f3, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-16.md
  notes: M4 reached. round 1 review FAIL (persisted sessions not listed as closed; failed --resume erased stored id) -> fixed in 6a3c8f3: Status::Closed (PROTOCOL_VERSION 4), launch::id_after_launch keeps id, per-entry tolerant state.json parse; first launch in a repo with no history: --continue fails then fresh (~2 s); non-zero exit <10 s before SessionStart = failed rung (Ctrl-C at trust dialog relaunches next rung); [sec Low] clean() lets bidi/zero-width (Cf) through; Closed rows display project/repo from state.json not config spec; restart after leader exit leaves group descendants; reap/unregister window; stale state.json.tmp.<pid> not cleaned; [sec Info] --continue fallback may pick another conversation in same repo
Task 17: started
Task 17: complete (221452a..11c8de7, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-17.md
  notes: M5 reached (fake-claude only; real claude not exercised). PROTOCOL_VERSION 5; pricing schema [pricing.models."<prefix>"] with deny_unknown_fields (old flat layout now a config error); no price -> cost n/a; transcript confined to <CLAUDE_CONFIG_DIR|~/.claude>/projects (O_NOFOLLOW+fstat+/proc/self/fd); [sec Low] add O_NOCTTY|O_CLOEXEC to transcript open (dir-component swap could make daemon open a tty -> controlling terminal -> SIGHUP); consider openat2 RESOLVE_BENEATH; [sec Low] dedup ids unbounded in length (cap 128 or hash); hard links in projects/ pass (nlink>1/uid check); [sec Info] sparse huge file busy-loops tailer; Cf chars in clean(); [review] tailer unit tests not run red first (mutation-checked)
Task 18: started
Task 18: complete (25cec55..535a0e9, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-18.md
  notes: M6 reached. Alt-n now focus-mode only (n in normal), per spec; bad keybindings fail the whole config load (TUI notice + defaults); only unfocus checked vs plain printable keys; [sec Low] relative repo path starting with '-' read as editor option — require absolute repo paths; [sec Low] editor program resolved after chdir into repo (relative program / '.' in PATH) — resolve against parent PATH first; [sec Info] Cf chars accepted as key specs; [review] e2e_keys not run red first
Task 19: started
Task 19: complete (1b92531..cf13e48, gate ok, security PASS, review PASS, evidence PROVEN)
  evidence: docs/plans/baton-mvp.evidence/task-19.md
  notes: controller re-check with REAL claude 2.1.295 (no tokens): evidence-collector's real-claude probe FAILed because its scratch dir was untrusted (trust dialog — doctor hint correct); in trusted /home/glepape/project/baton with CLAUDE_CONFIG_DIR=~/.claude-personal, 'baton doctor' printed PASS for config/dirs/command/version(2.1.295)/hooks/notify, exit 0, no probe left. FINDING: claude launched from inside a Claude Code session inherits CLAUDE_CODE_CHILD_SESSION and turns transcript saving off -> daemon should strip CLAUDE_CODE_* markers from session env; [sec Low] killpg after reap (WNOWAIT); Ctrl-C skips drop guards (no SIGINT handler); setsid'd descendants survive; Cf chars + profile name unsanitized; huge probe-timeout env panics; --version vs probe PATH resolution can differ; [review] profiles with no repo never checked
Final evidence: EVIDENCE: PROVEN (fake-claude end-to-end; real claude only via controller's baton doctor run in Task 19)
Final review: VERDICT: FAIL (1 critical) — daemon passes CLAUDECODE / CLAUDE_CODE_CHILD_SESSION / CLAUDE_CODE_* session markers to child claude, disabling transcript saving (usage + resume) when baton is started from inside Claude Code; warnings: inconsistent sanitize/clean helpers (Cf chars), duplicated unescape, combined hardening notes. Not fixed — awaiting user decision.
