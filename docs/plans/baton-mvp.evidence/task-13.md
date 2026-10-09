# Evidence — Task 13: Hook plumbing
Commit: ada5c45
Environment: debug build (`cargo build`), scratch dir `$D` (mktemp) holding BATON_CONFIG / BATON_STATE_DIR / BATON_RUNTIME_DIR / FAKE_CLAUDE_HOME / FAKE_CLAUDE_LOG; profile command = `target/debug/fake-claude`. Daemon started with `baton daemon start`, stopped at the end. Output below is real; paths shortened nowhere (D = /tmp/tmp.rmODMsBhze).

## hooks.json written at daemon start, 9 events, no matcher, quoted abs exe, timeout 5
Status: PROVEN
```console
$ baton daemon start; stat -c '%a %n' $D/run/hooks.json; jq '.hooks|keys' $D/run/hooks.json
daemon started pid=1121884
600 /tmp/tmp.rmODMsBhze/run/hooks.json
["Notification","PermissionRequest","PostToolUse","PreToolUse","SessionEnd","SessionStart","Stop","StopFailure","UserPromptSubmit"]
$ cat $D/run/hooks.json   (one line; the Stop entry shown, the others are identical except for the event name)
{"hooks":{... "Stop":[{"hooks":[{"command":"'/home/glepape/project/baton/target/debug/baton' hook Stop","timeout":5,"type":"command"}]}], ...}}
```
All 9 events present, mode 0600, command is `'<abs exe>' hook <Event>`, timeout 5, no "matcher" key. The "written atomically" and golden-file test parts are internal (covered by tests).

## Spawn argv: profile argv + `--settings <hooks.json>`
Status: PROVEN
```console
$ baton debug open x; cat $FAKE_CLAUDE_LOG
x//tmp/tmp.rmODMsBhze/a
{"argv":["/home/glepape/project/baton/target/debug/fake-claude","--settings","/tmp/tmp.rmODMsBhze/run/hooks.json"],"cwd":"/tmp/tmp.rmODMsBhze/a","env":{"BATON_SESSION":"x//tmp/tmp.rmODMsBhze/a","BATON_SOCK":"/tmp/tmp.rmODMsBhze/run/baton.sock","CLAUDE_CONFIG_DIR":"/home/glepape/.claude-personal"}}
```
The spawned process got `--settings <runtime>/hooks.json` and BATON_SESSION / BATON_SOCK. Resume flags are Task 16, not applicable.

## `baton hook` exits 0, silent, fast in all failure cases
Status: PROVEN
```console
$ echo '{"hook_event_name":"Stop"}' | BATON_SESSION=x BATON_SOCK=/nonexistent/sock baton hook Stop
rc=0 t=.004085736
$ printf garbage | (same env) baton hook Stop
rc=0 t=.003272054
$ BATON_SESSION=x baton hook Stop </dev/null          (empty stdin; real socket, session unknown)
rc=0 t=.003475170
$ echo '{}' | baton hook Stop                         (BATON_SESSION unset)
rc=0 t=.002518583
$ echo '{}' | BATON_SESSION=x BATON_SOCK=$D/nosock baton hook Stop   (regular file as socket)
rc=0 t=.003459164
$ head -c 3000000 /dev/zero | tr '\0' a | BATON_SESSION=x baton hook Stop   (3 MB stdin)
rc=0 t=.006813661
$ (sleep 3) | BATON_SESSION=x baton hook Stop          (slow stdin)
rc=0 t=.504047209
$ echo '{}' | baton hook ; echo '{}' | baton hook --bogus Stop extra     (bad args)
rc=0 t=.002768765
rc=0 t=.002604690
$ echo '{}' | BATON_SESSION=x BATON_SOCK=/nonexistent/sock BATON_HOOK_DEBUG=1 baton hook Stop 2>&1
baton hook: daemon is not running
rc=0 t=.003027766
```
Every case printed only the rc line (nothing from the hook on stdout/stderr), exit 0, under 10 ms, except slow stdin which stops at the 500 ms read deadline. Stderr appears only with BATON_HOOK_DEBUG=1. (Bad-args cases printed no clap error either.)

## Daemon handling: unknown BATON_SESSION dropped
Status: PROVEN (observable part)
```console
$ echo '{"hook_event_name":"Stop"}' | BATON_SESSION=nope//nowhere baton hook Stop
rc=0 t=.003242821
```
Hook for an unknown session exits 0 with no output and the daemon stayed up (next command worked). The debug-level log line was not inspected.

## e2e: SessionStart propagates claude_session_id / transcript_path / model
Status: PROVEN
```console
$ baton debug sessions --json | jq '.[0]'
{ "id": "x//tmp/tmp.rmODMsBhze/a", "project": "x", "profile": "p", "status": "Starting",
  "claude_session_id": "7f806d39-6b72-48b5-85e7-30320d271f1c",
  "transcript_path": "/tmp/tmp.rmODMsBhze/fch/projects/-tmp-tmp-rmODMsBhze-a/7f806d39-6b72-48b5-85e7-30320d271f1c.jsonl",
  "model": "claude-opus-5-5", ... }   (trimmed)
$ baton debug screen x//tmp/tmp.rmODMsBhze/a | head -3
DA1: ok
FAKE CLAUDE session=7f806d39-6b72-48b5-85e7-30320d271f1c model=claude-opus-5-5
>
$ find $FAKE_CLAUDE_HOME -type f
/tmp/tmp.rmODMsBhze/fch/projects/-tmp-tmp-rmODMsBhze-a/7f806d39-6b72-48b5-85e7-30320d271f1c.jsonl
```
claude_session_id equals the id fake-claude printed, transcript_path is under FAKE_CLAUDE_HOME, model propagated. (Status stays "Starting" because the state machine is Task 14.)

## Optional: real claude
Status: NOT PROVEN (skipped: a real claude start may hit trust/login prompts and would need Claude settings; not attempted)

## Cleanup
```console
$ baton daemon stop; pgrep -x baton; echo "pgrep rc=$?"
daemon stopped
pgrep rc=1
```
No baton or fake-claude process left; scratch dir removed; `git status --short` empty before writing this file.
