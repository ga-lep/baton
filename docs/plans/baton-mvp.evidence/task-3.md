# Evidence — Task 3: Test kit: baton-drive PTY driver and fake-claude
Commit: 88b4f84
Environment: `cargo build -p baton-testkit`; binaries run from target/debug with temp dirs in the session scratchpad (FAKE_CLAUDE_HOME, FAKE_CLAUDE_LOG, hooks file appending stdin to h.log). Observation: the PTY child's cwd is $HOME (/home/glepape), not the caller's cwd, so relative `--settings` paths fail; absolute paths were used. Not in the criteria.

## baton-drive CLI (--size, --timeout-ms, --step, send/wait/resize/sleep/dump/expect-exit, unescapes)
Status: PROVEN
```console
$ baton-drive --size 24x80 --step 'wait:DA1: ok' --step 'send:prompt hi\r' --step 'wait:> ' --step dump -- target/debug/fake-claude --model m1 --bogus
--- screen 24x80 ---
DA1: ok
FAKE CLAUDE session=e7b563be-3aea-493b-a7b9-2a096065deab model=m1
>
--- end screen ---
exit=0
$ baton-drive --timeout-ms 500 --step 'wait:NEVERMATCH' -- fake-claude; echo exit=$?
baton-drive: step "wait:NEVERMATCH": timed out after 500ms: waiting for /NEVERMATCH/
--- screen 24x80 ---
DA1: ok
FAKE CLAUDE session=fed9e81f-e620-48fd-84cc-75e3e91af416 model=claude-opus-5-5
>
--- end screen ---
exit=1
$ baton-drive --step 'wait:DA1' --step 'send:exit 2\r' --step expect-exit:0 -- fake-claude; echo exit=$?
baton-drive: step "expect-exit:0": expected exit 0, got 2
exit=1
$ baton-drive --step 'send:echo a\tb\x41\\\r' --step 'sleep:300' --step resize:10x40 --step 'send:tput cols;tput lines\r' --step 'sleep:300' --step dump -- bash --norc --noprofile
--- screen 10x40 ---
echo a  bA\
bash-5.2$ echo abA\
> tput cols;tput lines
abAtput cols
10
bash-5.2$
--- end screen ---
```
Hint command gives a screen with `FAKE CLAUDE session=` and `DA1: ok` (so the responder answers DA1); wait timeout prints the screen to stderr (stdout/stderr merged here) and exits 1; dump uses the markers; \t, \x41, \\ and \r unescape; resize takes effect (10 lines seen; the width 40 line is scrolled/garbled by the unclean bash send, see the smoke test below for tput cols). Per-step timeout flag works. Note: the wait for `> ` in the hint matched immediately.

## fake-claude arguments and launch log
Status: PROVEN
```console
$ cat $FAKE_CLAUDE_LOG
{"argv":["/home/glepape/project/baton/target/debug/fake-claude","--model","m1","--bogus"],"cwd":"/home/glepape","env":{"BATON_SESSION":"sess1","BATON_SOCK":"/x/sock","CLAUDE_CONFIG_DIR":"/home/glepape/.claude-personal"}}
{"argv":["fake-claude","--settings",".../w/hooks.json","--session-id","11111111-2222-4333-8444-555555555555","--model","m1"],"cwd":"/home/glepape","env":{...same...}}
```
Unknown `--bogus` ignored; one JSON line per launch with argv, cwd, the three env vars; `--session-id` honoured (screen shows that id).

## fake-claude startup (DA1, banner, prompt, transcript path)
Status: PROVEN
```console
$ timeout 5 fake-claude </dev/null | head -3     # no terminal reply
DA1: timeout
FAKE CLAUDE session=09c69433-a911-4ab5-af07-3fc6bfda2935 model=claude-opus-5-5
>
$ find (FAKE_CLAUDE_HOME=$W/home) -type f
home/projects/-home-glepape/e7b563be-3aea-493b-a7b9-2a096065deab.jsonl
home/projects/-home-glepape/11111111-2222-4333-8444-555555555555.jsonl
$ CLAUDE_CONFIG_DIR=$W/cc (FAKE_CLAUDE_HOME unset) ... ; find cc -type f
cc/projects/-home-glepape/2013c822-50ec-4dbf-8270-636043421222.jsonl
$ env -u CLAUDE_CONFIG_DIR HOME=$W/hh ... ; find hh -type f
hh/.fake-claude/projects/-tmp-claude-...-scratchpad-w-hh/ae6e18e9-1078-4e1e-8228-d16871ab0237.jsonl
```
DA1 ok under the driver and timeout without a responder; banner and `> ` shown; transcript at projects/<slug>/<id>.jsonl; home precedence FAKE_CLAUDE_HOME, CLAUDE_CONFIG_DIR, ~/.fake-claude all verified. A fresh launch without --session-id gets a uuid v4-looking id.

## fake-claude hooks (--settings, SessionStart source, NO_HOOKS)
Status: PROVEN
```console
$ head -1 h.log | jq -c .
{"cwd":"/home/glepape","hook_event_name":"SessionStart","model":"m1","permission_mode":"default","scratchpad_dir":".../home/scratch","session_id":"11111111-2222-4333-8444-555555555555","source":"startup","transcript_path":".../home/projects/-home-glepape/11111111-2222-4333-8444-555555555555.jsonl"}
$ (--resume 1111... --settings hooks.json); jq -c '[.hook_event_name,.source]' h.log
["SessionStart","resume"]
["SessionEnd",null]
$ FAKE_CLAUDE_NO_HOOKS=1 baton-drive ... send:prompt x ... ; wc -c h.log
0 h.log
```
Hooks run via sh -c with event JSON on stdin; source is startup then resume; NO_HOOKS disables firing.

## fake-claude resume rules
Status: PROVEN
```console
$ fake-claude --resume nope; echo exit=$?
No conversation found with session ID: nope
exit=1
$ (cd empty; FAKE_CLAUDE_HOME=<fresh> fake-claude --continue; echo exit=$?)
No conversation found to continue
exit=1
$ baton-drive ... -- fake-claude --resume 11111111-... ; echo exit=$?     # known id
(banner with session=11111111-..., exit=0)
$ baton-drive ... -- fake-claude --continue ; echo exit=$?                  # prior transcript exists for cwd
exit=0   (SessionStart source=resume)
```

## fake-claude input lines
Status: PROVEN
Session: prompt hi, perm, allow, idle, color, exit 3 (baton-drive `expect-exit:3` passed, exit=0).
```console
screen: > prompt hi / done / > perm / permission needed / > allow / > idle / > color / red text
$ jq -c '[.hook_event_name,.source,.notification_type,.reason]' h.log
["SessionStart","startup",null,null]
["UserPromptSubmit",null,null,null]
["PreToolUse",null,null,null]
["PostToolUse",null,null,null]
["Stop",null,null,null]            <- prompt
["PreToolUse",null,null,null]
["PermissionRequest",null,null,null]
["Notification",null,"permission_prompt",null]   <- perm
["PostToolUse",null,null,null]
["Stop",null,null,null]            <- allow
["Notification",null,"idle_prompt",null]         <- idle
["SessionEnd",null,null,"prompt_input_exit"]     <- exit 3
$ cat transcript | jq -c .   (trimmed to key fields)
two assistant entries, same message.id msg_8e44bb63..., model "m1", usage {input_tokens:100, output_tokens:50, cache_read_input_tokens:1000, cache_creation_input_tokens:10}
$ printf 'color\nexit 0\n' | fake-claude | od -c
> 033 [ 3 1 m r e d   t e x t 033 [ 0 m
```
All hook orders, usage values, SGR red and exit code match the table. (Default model claude-opus-5-5 seen in banner when --model omitted.)

## drive_smoke.rs
Status: PROVEN
```console
$ cargo test -p baton-testkit --test drive_smoke
test result: ok. 5 passed; 0 failed
```
Covers bash echo/resize/tput cols, SessionStart JSON fields, DA1 ok.

## Verdict
EVIDENCE: PROVEN
