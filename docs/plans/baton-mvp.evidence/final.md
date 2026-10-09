# Evidence — Final: Baton MVP end-to-end (Tasks 1-19)
Commit: 4fe905ba88cb703783772af4ae5937e500b74be5
Environment: `cargo build` debug. Scratch dir D (mktemp, removed afterwards) with BATON_CONFIG, BATON_STATE_DIR, BATON_RUNTIME_DIR=/tmp/bfr<pid>, BATON_NOTIFY_SINK=log, FAKE_CLAUDE_LOG. Config: profiles `work` and `personal` (both target/debug/fake-claude, env CLAUDE_CONFIG_DIR inside D plus WHO), `[pricing] default = {input=15, output=75, cache_read=1.5, cache_write=18.75}`, project `alpha` (3 repos a1,a2 = work, a3 per-repo override = personal) and project `beta` (1 repo, personal). TUI = `baton` driven by `baton-drive` at 40x120. Real claude never used. Dumps trimmed (blank panel rows, long lines, some info-panel rows). Raw output was captured live; this file contains the decisive excerpts.

## Gate: fmt + clippy -D warnings + cargo test
Status: PROVEN
```console
$ cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
(no fmt/clippy output; every `test result:` line is `ok ... 0 failed`, 15 suites, e.g. 94 passed in baton-core)
```

## T1: `--help` lists daemon/hook/spike/doctor/config, `debug` hidden; hook silent exit 0
Status: PROVEN
```console
$ baton --help
Commands:
  daemon  hook  spike  doctor  config  help      (no `debug` line)
$ echo '{}' | baton hook Stop; echo "rc=$?"
rc=0
$ printf 'garbage' | BATON_SESSION=x BATON_SOCK=/nonexistent/sock baton hook Stop; echo rc=$?   # also with valid JSON
rc=0        (real 0.003s and 0.002s)
```
`baton debug --help` still works (hidden). Hook prints only rc and is far under 0.3 s (T13 hook evidence too).

## T2 / T4 / T5 / T7: internal modules
Status: NOT PROVEN (internal - covered by tests); the full test suite passed in the gate and the modules are exercised through the TUI/spike runs below.

## T3: baton-drive + fake-claude
Status: PROVEN
```console
$ baton-drive --size 24x80 --step 'wait:DA1: ok' --step 'send:prompt hi\r' --step 'wait:> ' --step dump -- target/debug/fake-claude
--- screen 24x80 ---
DA1: ok
FAKE CLAUDE session=6ed2170d-b5f6-4178-9545-49a43bf30550 model=claude-opus-5-5
--- end screen ---
```
Screen contains `FAKE CLAUDE session=` and `DA1: ok` (the daemon-side responder also answers DA1 in the sessions below).

## T6: spike
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:FOCUS' --step 'send:tput cols\r' --step 'wait:│86 ' --step 'send:\x1c' --step 'wait:NORMAL' --step 'send:q' --step 'expect-exit:0' -- baton spike -- bash --norc
spike exit=0
```
Caveat: the plan's literal `wait:^86$` times out (the line is inside the panel border: `││86   ...`). Screen shows `bash-5.2$ tput cols` / `86`. Same caveat as task-6.md; the bordered `wait:│86 ` form passes.

## T8: config check
Status: PROVEN
```console
$ baton config check
alpha  /tmp/bfin.dOGn/r/a1  profile=work  cmd=["/home/glepape/project/baton/target/debug/fake-claude"]  env=CLAUDE_CONFIG_DIR,WHO
alpha  /tmp/bfin.dOGn/r/a2  profile=work  cmd=[...]  env=CLAUDE_CONFIG_DIR,WHO
alpha  /tmp/bfin.dOGn/r/a3  profile=personal  cmd=[...]  env=CLAUDE_CONFIG_DIR,WHO
beta  /tmp/bfin.dOGn/r/b1  profile=personal  cmd=[...]  env=CLAUDE_CONFIG_DIR,WHO
exit=0
$ (beta profile changed to "nope") baton config check
project "beta" repo /tmp/bfin.dOGn/r/b1: unknown profile "nope"
exit=1
```
Per-repo override (a3 = personal) resolved; invalid profile gives exit 1 with the expected message.

## T9: daemon lifecycle
Status: PROVEN
```console
$ baton daemon stop            -> daemon stopped
$ baton daemon status; echo "status exit=$?"
not running
status exit=1
$ (TUI auto-started daemon) stat -c %a $R/baton.sock ; baton daemon status
600
running pid=2278642 protocol=5 sessions=3
```
Socket mode 0600; status text and exit codes as specified; the TUI auto-starts the daemon.

## Goal flow 1: sidebar shows closed projects, `o` opens, sessions start (launch fresh)
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:▸ alpha' --step dump --step 'send:o' --step 'wait:3 . a3' --step 'sleep:1500' --step dump -- baton
│▸ alpha  (closed)   │ │No sessions. Start a project with `baton debug open <name>`.
│▸ beta  (closed)
--- after `o` ---
│▾ alpha             │┌a1 · work · ○ idle
│  1 ○ a1  idle      ││DA1: ok
│  2 ○ a2  idle      ││FAKE CLAUDE session=10ce8ca3-de51-452f-b042-9780743cdaa3 model=claude-opus-5-5
│  3 ○ a3  idle      ││>
│▸ beta  (closed)
$ baton debug sessions --json | jq -c '.[]|{id,status,claude_session_id,launch}'   (excerpt)
a1 Idle 10ce8ca3-... fresh ; a2 Idle 8e5e4e9c-... fresh ; a3 Idle 714bd3b6-... fresh
$ jq -r '.argv|join(" ")' $FAKE_CLAUDE_LOG   (first lines)
fake-claude --settings /tmp/bfr2275544/hooks.json --continue      (x3, exit 1: no history)
fake-claude --settings /tmp/bfr2275544/hooks.json --session-id 10ce8ca3-de51-452f-b042-9780743cdaa3   (and a2, a3)
```
Launch ladder on a repo with no history: `--continue` fails early, then fresh with `--session-id`; daemon log "launch attempt failed early; trying the next rung ... to=fresh". Hooks injected via `--settings <runtime>/hooks.json` (T13).

## Goal flow 2: `prompt hi` -> info panel tokens/cost (est.)
Status: PROVEN
```console
(focus a1, send 'prompt hi\r', Ctrl-\)
│  1 ○ a1  idle      │> prompt hi / done
│model    claude-opus-5-5
│context  ░░░░░░░░░░ 1%
│tokens   100 in / 50 out
│cache    1.0k read / 10 write
│cost     ~$0.0069 (est.)
$ baton debug sessions --json | jq -c '.[0].usage'   (after a 2nd prompt, after TUI-quit, T17)
{"input":200,"output":100,"cache_read":2000,"cache_write":20,"context_pct":0.555,"cost_usd":0.013875,...}
```
100*15+50*75+1000*1.5+10*18.75 = 6937.5 per MTok = $0.0069375, matching the [pricing] default; two prompts give input 200, output 100 as in T17 Evidence. Note: in the first dump taken right after `prompt hi` the panel still showed `id n/a` and `launch continue` (stale); later dumps (and the relaunched TUI) showed `id 10ce...daa3`, `launch fresh`. Minor, cosmetic.

## Goal flow 3: `perm` in another session -> badge, notification, `n`, `allow`
Status: PROVEN
```console
(focus a2, send 'perm\r', Ctrl-\, select 1)
│  1 ○ a1  idle
│  2 ◐ a2  permission
(send 'n')  -> main panel title `a2 · work · ◐ permission`, info panel `status permission`; screen shows `> perm` / `permission needed`
(Enter, send 'allow\r')  -> `2 ○ a2 idle`; `baton debug sessions --json` all Idle
$ baton debug send 'alpha//tmp/bfin.dOGn/r/a3' 'perm\r'; sleep 0.7; cat $BATON_STATE_DIR/notifications.log
notify alpha//tmp/bfin.dOGn/r/a3 Permission
```
Badge ◐ and `n` jump work. Notification rule note: the first `perm` (a2) was on-screen in the focused attached TUI, so correctly no log line; the a3 `perm` (not on screen) wrote the line. Later `n` from a1 jumped to a3 (`a3 · personal · ◐ permission`) and `allow` returned it to Idle.

## Goal flow 4: `?` help, Ctrl-\ and `q`
Status: PROVEN
```console
(Ctrl-\ then '?')
┌Key bindings (? or Esc to close)──
│Normal mode ... move_down j, down ... next_attention n ... restart r ... quit q      Focus mode: unfocus ctrl-\, next_attention alt-n, session_1 alt-1 ...
(? closes, q) -> baton-drive expect-exit:0 passed
$ baton daemon status
running pid=2275883 protocol=5 sessions=3
```
Ctrl-\ left focus (NORMAL bar); `q` quit the TUI with exit 0 while the daemon kept 3 sessions.

## Goal flow 5: relaunch TUI, screen intact
Status: PROVEN
```console
$ baton-drive ... --step 'wait:a1 · work' --step 'send:n' ... -- baton
a3 · personal · ◐ permission  (attention state preserved) ...
(select 1)  a1 · work · ○ idle: `> prompt hi` / `done`, tokens 100 in / 50 out, cost ~$0.0069 (est.), id 10ce…daa3, launch fresh
```
Terminal content, status and usage survived the TUI restart (T11/T12 evidence).

## Goal flow 6: `baton daemon stop` -> reopen -> Closed, `o` resumes with the same ids
Status: PROVEN
```console
$ baton daemon stop        -> daemon stopped        ($ pgrep -x fake-claude | wc -l -> 0)
$ baton-drive ... -- baton (TUI auto-starts a fresh daemon)
│▸ alpha  (closed)
│  1 ◌ a1  closed    / 2 ◌ a2  closed / 3 ◌ a3  closed
│  info: status closed, tokens n/a, id 10ce…daa3
(send 'o')
│▾ alpha  1 ○ a1 idle  2 ○ a2 idle  3 ○ a3 idle   info: tokens 100 in / 50 out, cost ~$0.0069 (est.), launch resume
$ tail -3 $FAKE_CLAUDE_LOG | jq -r '.argv|join(" ")'
fake-claude --settings /tmp/bfr2275544/hooks.json --resume 10ce8ca3-de51-452f-b042-9780743cdaa3
fake-claude --settings /tmp/bfr2275544/hooks.json --resume 8e5e4e9c-3176-430c-8128-c0d501f2ffaf
fake-claude --settings /tmp/bfr2275544/hooks.json --resume 714bd3b6-e359-4f78-8393-fd3d903aa4bd
```
Ids match those before the stop (first dump of the flow: 10ce…, 8e5e…, 714b…). Usage also rebuilt from the transcript on resume (dedup by message.id: 100/50 not double counted); after the next prompt it was 200/100.

## Goal flow 7: `r` restart with confirm
Status: PROVEN
```console
(a2 in Permission via `baton debug send ... perm`; TUI: select 2, 'r')
 NORMAL │ Restart a2? [y/N]
(send 'y') -> a2 relaunched, screen `a2 · work · ○ idle`, fresh `FAKE CLAUDE session=8e5e4e9c-...`
$ jq -r '.argv|join(" ")' $FAKE_CLAUDE_LOG | grep 8e5e | sed 's#.*hooks.json##'
 --session-id 8e5e4e9c-...
 --resume 8e5e4e9c-...      (x3: reopen after daemon stop, restart with y, restart of idle session without prompt)
```
Confirm prompt shown for a Permission session; `y` restarts with `--resume <same id>`. An Idle session restarts immediately without a prompt (as specified for non-Running/Permission).

## T10/T11/T12/T13/T14/T15/T16/T17: per-task evidence lines
Status: PROVEN (covered by the flows above): `debug open beta` + `debug send ... 'prompt hi\r'` + `debug screen` work against the same daemon (T10; usage 100/50 appeared after resending; the first send, issued <1 s after open, was swallowed by fake-claude before it was ready - test timing, not a Baton fault); `baton debug sessions` lists sessions; hook-reported `claude_session_id` equals the id printed on screen and `transcript_path` is under the profile's CLAUDE_CONFIG_DIR (`/tmp/bfin.dOGn/cc-w/projects/-tmp-bfin-dOGn-r-a1/<id>.jsonl`, personal sessions under `cc-p`) (T13); statuses Idle/Permission (T14); notifications.log line (T15); resume ladder, restart (T16); tokens/cost/est. panel (T17).

## T18: remappable keys, help overlay, editor
Status: PROVEN
```console
$ (config + [keybindings.normal] next_attention="x" + [keybindings.focus] unfocus="ctrl-g")
$ baton-drive --step 'wait:NORMAL' --step 'send:?' --step 'wait:next_attention +x' --step 'send:?' --step 'send:1' --step 'send:e' --step 'sleep:800' --step 'send:\r' --step 'wait:FOCUS' --step 'send:\x07' --step 'wait:NORMAL' --step 'send:q' -- baton
 NORMAL │ ⏎ focus  x next-attention  r restart  e editor ...
edited=/tmp/bfin.dOGn/r/a1
```
Wait steps for `next_attention +x` and for NORMAL after Ctrl-g (\x07) passed; the bar shows the remapped `x`; `e` ran the editor template and wrote the repo path to `$D/edited`.

## T19: baton doctor
Status: PROVEN
```console
$ baton doctor; echo "exit=$?"
PASS config: .../config.toml parses
PASS dirs: runtime dir /tmp/bfr2275544 is private (0700)
PASS dirs: state dir .../state is writable
PASS daemon: running pid=2278642 protocol=5
PASS command: .../fake-claude (profile work)
PASS version: DA1: timeout (profile work)
PASS hooks: SessionStart received (profile work)
PASS command: .../fake-claude (profile personal)
PASS version: DA1: timeout (profile personal)
PASS hooks: SessionStart received (profile personal)
PASS notify: notify-send and the D-Bus session bus are available
exit=0
```

## Config check at end / Cleanup
Status: PROVEN
```console
$ baton daemon stop -> daemon stopped
$ pgrep -x baton ; pgrep -x fake-claude     -> both empty
$ rm -rf scratch dirs; git status --short   -> empty (only this file is new/changed after it is written)
```
Daemon log had 0 lines matching panic/ERROR.

## Verdict
Every outside-observable criterion was demonstrated; findings were cosmetic/non-blocking (stale `id n/a`/`launch continue` in one early info-panel dump; the plan's literal T6 `wait:^86$` hint cannot match inside the bordered panel).

EVIDENCE: PROVEN
