# Evidence — Task 16: Persistence, launch ladder and restart
Commit: 6a3c8f3
Environment: debug build (cargo build); scratch dir /tmp/bev16 (BATON_CONFIG/STATE_DIR/RUNTIME_DIR, FAKE_CLAUDE_HOME=/tmp/bev16/fc, FAKE_CLAUDE_LOG=/tmp/bev16/launch.log, BATON_NOTIFY_SINK=off); profile command target/debug/fake-claude; TUI driven with target/debug/baton-drive (PTY, 30x100). $B = target/debug/baton, SID = x//tmp/bev16/r. Outputs below are verbatim from the runs (the TUI dumps were filtered with grep to the relevant lines).

## state.json format, 0600 mode, fresh first launch (continue fails -> fresh --session-id), daemon stop + reopen -> --resume <same id>
Status: PROVEN

```console
$ baton debug open x
x//tmp/bev16/r
exit=0
{"id":"x//tmp/bev16/r","project":"x","repo":"/tmp/bev16/r","profile":"p","status":"Idle","claude_session_id":"5862eeb9-7c24-4446-bb4e-42d364b6c32c","transcript_path":"/tmp/bev16/fc/projects/-tmp-bev16-r/5862eeb9-7c24-4446-bb4e-42d364b6c32c.jsonl","model":"claude-opus-5-5","started_at":1791515306,"exit_code":null,"usage":null,"launch":"fresh"}
$ launch log
["/home/glepape/project/baton/target/debug/fake-claude","--settings","/tmp/bev16/run/hooks.json","--continue"]
["/home/glepape/project/baton/target/debug/fake-claude","--settings","/tmp/bev16/run/hooks.json","--session-id","5862eeb9-7c24-4446-bb4e-42d364b6c32c"]
$ send prompt hi
exit=0
{"status":"YourTurn","launch":"fresh","claude_session_id":"5862eeb9-7c24-4446-bb4e-42d364b6c32c"}
ID=5862eeb9-7c24-4446-bb4e-42d364b6c32c
$ state.json
600 /tmp/bev16/state/state.json
{
  "version": 1,
  "sessions": {
    "x//tmp/bev16/r": {
      "project": "x",
      "repo": "/tmp/bev16/r",
      "profile": "p",
      "claude_session_id": "5862eeb9-7c24-4446-bb4e-42d364b6c32c",
      "transcript_path": "/tmp/bev16/fc/projects/-tmp-bev16-r/5862eeb9-7c24-4446-bb4e-42d364b6c32c.jsonl",
      "last_status": "YourTurn",
      "updated_at": 1791515310
    }
  }
}
$ daemon stop
daemon stopped
exit=0
$ pgrep after stop
$ debug open x
x//tmp/bev16/r
/home/glepape/project/baton/target/debug/fake-claude --settings /tmp/bev16/run/hooks.json --resume 5862eeb9-7c24-4446-bb4e-42d364b6c32c
{"status":"Idle","launch":"resume","claude_session_id":"5862eeb9-7c24-4446-bb4e-42d364b6c32c"}
```
state.json has the specified fields, mode 600; first launch --continue then --session-id <id> (info 'launch: fresh'); after daemon stop, reopen launches --resume with the same id and launch=resume.

## Persisted sessions listed Closed after daemon restart until project opened; TUI shows it under the closed project
Status: PROVEN

```console
daemon stopped
$ sessions after daemon restart (no open)
3
{"status":"Closed","launch":null,"claude_session_id":"5862eeb9-7c24-4446-bb4e-42d364b6c32c","project":"x"}
3
$ TUI
--- screen 30x100 ---
┌Projects──────────────────────┐┌r · p · ◌ closed──────────────────────────────────────────────────┐
│▸ x  (closed)                 ││No sessions. Start a project with `baton debug open <name>`.      │
│  1 ◌ r  closed               ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
└──────────────────────────────┘│                                                                  │
┌Session───────────────────────┐│                                                                  │
│/tmp/bev16/r                  ││                                                                  │
│profile  p                    ││                                                                  │
│status  closed                ││                                                                  │
│uptime  8s                    ││                                                                  │
│                              ││                                                                  │
│                              ││                                                                  │
└──────────────────────────────┘└──────────────────────────────────────────────────────────────────┘
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  ? help  q quit
--- end screen ---
4
/home/glepape/project/baton/target/debug/fake-claude --settings /tmp/bev16/run/hooks.json --resume 5862eeb9-7c24-4446-bb4e-42d364b6c32c
```
Closed status with same claude_session_id, launch log unchanged (3 lines) by listing; TUI shows '▸ x (closed)' / '1 ◌ r closed'; pressing o launches --resume.

## Restart: TUI r with y/N confirm on a Permission session, relaunch with --resume; info panel 'launch resume'
Status: PROVEN

```console
{"status":"Idle","launch":"resume"}
{"status":"Permission","launch":"resume"}
4
$ TUI: r then n
1:--- screen 30x100 ---
13:│status  permission            ││                                                                  │
15:│launch  resume                ││                                                                  │
17: NORMAL │ Restart r? [y/N]
18:--- end screen ---
19:--- screen 30x100 ---
31:│status  permission            ││                                                                  │
33:│launch  resume                ││                                                                  │
35: NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  ? help  q quit
36:--- end screen ---
37:baton-drive: step "wait:launch resume": timed out after 10s: waiting for /launch resume/
38:--- screen 30x100 ---
48:│status  idle                  ││                                                                  │
50:│launch  resume                ││                                                                  │
52: NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  ? help  q quit
53:--- end screen ---
5
/home/glepape/project/baton/target/debug/fake-claude --settings /tmp/bev16/run/hooks.json --resume 5862eeb9-7c24-4446-bb4e-42d364b6c32c
{"status":"Idle","launch":"resume","claude_session_id":"5862eeb9-7c24-4446-bb4e-42d364b6c32c"}
```
r on Permission shows 'Restart r? [y/N]'; n leaves the session untouched (launch log stayed at 4 lines); r then y added exactly one launch with --resume <same id>; panel shows 'launch  resume'. (One baton-drive wait step timed out because my regex used one space where the panel has two; the later dump shows the state.)

## debug restart relaunches with --resume; unknown session errors; deleting the transcript walks resume -> continue -> fresh; corrupt state.json renamed to .bad-<ts> and daemon starts
Status: PROVEN

```console
$ debug restart
exit=0
/home/glepape/project/baton/target/debug/fake-claude --settings /tmp/bev16/run/hooks.json --resume 5862eeb9-7c24-4446-bb4e-42d364b6c32c
{"status":"Idle","launch":"resume"}
baton debug: unknown session "nope//x"
exit=1
$ transcript delete
daemon stopped
x//tmp/bev16/r
["/tmp/bev16/run/hooks.json --resume 5862eeb9-7c24-4446-bb4e-42d364b6c32c"]
["/tmp/bev16/run/hooks.json --continue"]
["/tmp/bev16/run/hooks.json --session-id 2ae3ee54-bf2b-4a7b-be75-5a8c093abe30"]
{"argv":["/home/glepape/project/baton/target/debug/fake-claude","--settings","/tmp/bev16/run/hooks.json","--resume","5862eeb9-7c24-4446-bb4e-42d364b6c32c"],"cwd":"/tmp/bev16/r","env":{"BATON_SESSION":"x//tmp/bev16/r","BATON_SOCK":"/tmp/bev16/run/baton.sock","CLAUDE_CONFIG_DIR":"/home/glepape/.claude-personal"}}
{"argv":["/home/glepape/project/baton/target/debug/fake-claude","--settings","/tmp/bev16/run/hooks.json","--continue"],"cwd":"/tmp/bev16/r","env":{"BATON_SESSION":"x//tmp/bev16/r","BATON_SOCK":"/tmp/bev16/run/baton.sock","CLAUDE_CONFIG_DIR":"/home/glepape/.claude-personal"}}
{"argv":["/home/glepape/project/baton/target/debug/fake-claude","--settings","/tmp/bev16/run/hooks.json","--session-id","2ae3ee54-bf2b-4a7b-be75-5a8c093abe30"],"cwd":"/tmp/bev16/r","env":{"BATON_SESSION":"x//tmp/bev16/r","BATON_SOCK":"/tmp/bev16/run/baton.sock","CLAUDE_CONFIG_DIR":"/home/glepape/.claude-personal"}}
{"status":"Idle","launch":"fresh","claude_session_id":"2ae3ee54-bf2b-4a7b-be75-5a8c093abe30"}
$ corrupt state
daemon stopped
x//tmp/bev16/r
total 12
-rw-rw-r-- 1 glepape glepape 3596 Oct  9 05:09 daemon.log
-rw------- 1 glepape glepape  377 Oct  9 05:09 state.json
-rw------- 1 glepape glepape   19 Oct  9 05:09 state.json.bad-1791515363
{"status":"Idle","launch":"continue"}
baton.lock
baton.sock
hooks.json
/tmp/bev16/state/daemon.log
```
debug restart -> --resume <id>; unknown id exits 1. After deleting transcripts the three new launches are --resume <id>, --continue, --session-id <new uuid> and the session ends launch=fresh. Corrupt state.json was moved to state.json.bad-1791515363 (mode 600) and the daemon started and opened the session (launch=continue, as no id was known).

## Corrupt state is logged
Status: PROVEN

```console
2026-10-09T03:09:23.351352Z  WARN baton::daemon::persist: ignoring unusable state file, starting empty: /tmp/bev16/state/state.json: invalid JSON: key must be a string at line 1 column 3; moved to /tmp/bev16/state/state.json.bad-1791515363
$ final stop
daemon stopped
pgrep baton=1
pgrep fake-claude=1
```
daemon.log WARN line records the set-aside; final daemon stop leaves no baton or fake-claude process (pgrep exit 1).

## Not exercised from outside
- Debounce to 250 ms, atomic tmp+fsync+rename, SIGHUP then SIGKILL after 3 s, the 10 s early-failure window: Status: NOT PROVEN (internal — covered by tests).
- Restart of an Exited session: covered by e2e_resume.rs, not run here.

## Verdict
EVIDENCE: PROVEN
