# Evidence — Task 17: Transcript tailer, usage and cost, info panel
Commit: 11c8de7
Environment: target/debug binaries; scratch BATON_CONFIG/STATE/RUNTIME (runtime under /tmp/bt.*), BATON_NOTIFY_SINK=off, profile command = target/debug/fake-claude, CLAUDE_CONFIG_DIR set in profile env, `[pricing] default = { input = 3.0, output = 15.0, cache_read = 0.3, cache_write = 3.75 }`. Scratch dir is `$D` (paths abbreviated). Daemon stopped at end; `pgrep -x baton` and `pgrep -x fake-claude` empty. `~/.baton-ev17` removed. Real claude not used.

## e2e: `prompt hi` twice gives in=200 out=100 cache_read=2000 cache_write=20; counted once per message.id; context % and cost match formulas
Status: PROVEN
```console
$ baton debug open x; sleep 1.5; baton debug sessions --json | jq -c '.[0]|{status,usage}'   # before prompts
{"id":"x/$D/r","status":"Idle","usage":{"input":0,"output":0,"cache_read":0,"cache_write":0,"context_pct":null,"cost_usd":0.0,"model":null}}
$ baton debug send "x/$D/r" 'prompt hi\r'; sleep 1.5; baton debug send "x/$D/r" 'prompt hi\r'; sleep 1.5
$ baton debug sessions --json | jq '.[0].usage'
{
  "input": 200,
  "output": 100,
  "cache_read": 2000,
  "cache_write": 20,
  "context_pct": 0.555,
  "cost_usd": 0.002775,
  "model": "claude-opus-5-5"
}
```
Context % = (100+1000+10)/200000 = 0.555%. Cost = (200*3 + 100*15 + 2000*0.3 + 20*3.75)/1e6 = 0.002775. Both match. Totals are 200/100 for two prompts, so each message.id is counted once (hint says each prompt writes two entries). The transcript was found under `<CLAUDE_CONFIG_DIR>/projects/...jsonl`.

## CLAUDE_CONFIG_DIR with `~` is expanded at config level
Status: PROVEN
```console
$ grep CLAUDE config.toml
env = { CLAUDE_CONFIG_DIR = "~/.baton-ev17/claude" }
$ (open, send 'prompt hi\r' twice) ; baton debug sessions --json | jq -c '.[0].usage'
{"input":200,"output":100,"cache_read":2000,"cache_write":20,"context_pct":0.555,"cost_usd":0.002775,"model":"claude-opus-5-5"}
$ find ~/.baton-ev17 -name '*.jsonl'
/home/glepape/.baton-ev17/claude/projects/-tmp-...-r/11316ae8-1863-4094-909b-dcd4c02fad6f.jsonl
```
Usage was read from the transcript in the expanded home dir.

## Info panel (spec §4): context bar, tokens, cache line, cost `~$ (est.)`, abbreviated id, launch rung, uptime
Status: PROVEN
```console
$ baton-drive --size 40x120 --timeout-ms 15000 --step 'wait:est\.' --step dump -- target/debug/baton   # Session panel excerpt
┌Session───────────────────────┐
│…8/scratchpad/tmp.16Q1EoBuff/r│
│profile  p                    │
│model    claude-opus-5-5      │
│status   your turn            │
│uptime   9s                   │
│context  ░░░░░░░░░░ 1%        │
│tokens   200 in / 100 out     │
│cache    2.0k read / 20 write │
│cost     ~$0.0028 (est.)      │
│id       df49…2425            │
│launch   fresh                │
└──────────────────────────────┘
```
All fields present (the path is truncated with an ellipsis to fit the panel width). Exit code only shows for exited sessions, and I did not exercise that. The `1h12m` uptime format was not seen live, only `9s`.

## Transcript outside CLAUDE_CONFIG_DIR gives usage null / `n/a`; tailer never affects status
Status: PROVEN
```console
$ FAKE_CLAUDE_HOME=$D/elsewhere  (profile CLAUDE_CONFIG_DIR=$D/claude); baton debug send ... 'prompt hi\r'
$ baton debug sessions --json | jq -c '.[0]|{status,usage}'
{"status":"YourTurn","usage":null}
$ find $D/elsewhere -name '*.jsonl'
$D/elsewhere/projects/-tmp-...-r/31d73787-b7e9-4de6-8f7e-fd930152e135.jsonl
TUI Session panel:
│context  n/a                  │
│tokens   n/a                  │
│cache    n/a                  │
│cost     n/a                  │
```
The transcript exists but sits outside the profile dir. Usage is null, the panel shows `n/a`, and status still progressed to YourTurn.

## UsageAccumulator::feed_line rules (ignore non-assistant/unparsable lines, dedupe by id, sum sidechain, context from latest non-sidechain, model from latest)
Status: NOT PROVEN (internal — covered by tests). The dedupe and model parts are indirectly visible in the e2e output above.

## Context window configurable per model prefix, default 200 000
Status: NOT PROVEN (internal — covered by tests). The default of 200 000 is consistent with the 0.555% above. The per-prefix override was not exercised.

## Cost: longest-prefix match over `[pricing.models."<prefix>"]`, fallback to default, $/MTok
Status: NOT PROVEN (internal — covered by tests). The default fallback is shown in the e2e cost above. Longest-prefix matching was not exercised.

## Tailer: 1 s poll, partial-line buffering, silent wait for missing file, reset on truncation/path change, emit only on change
Status: NOT PROVEN (internal — covered by tests). Only the missing-file-then-appears and update-on-change behaviour were seen indirectly: usage appeared within 1.5 s of each prompt.

## Rendering unit-tested with TestBackend (full and n/a)
Status: NOT PROVEN (internal — covered by tests). Both states were seen live above.

## Cleanup
```console
$ baton daemon stop
daemon stopped
$ pgrep -x baton; pgrep -x fake-claude    # no output
$ ls -d ~/.baton-ev17
ls: cannot access '/home/glepape/.baton-ev17': No such file or directory
```

## Verdict
EVIDENCE: PROVEN
