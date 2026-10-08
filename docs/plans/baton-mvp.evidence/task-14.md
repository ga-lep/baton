# Evidence — Task 14: Status state machine, attention and `n` / `Alt-n`
Commit: 363280e
Environment: target/debug/baton driven through target/debug/baton-drive (40x120 PTY) with scratch BATON_CONFIG / BATON_STATE_DIR / BATON_RUNTIME_DIR / FAKE_CLAUDE_HOME in a mktemp dir; profile command = target/debug/fake-claude; project `x` with 2 repos (alpha, beta); hook_timeout_secs=20 (1 for the unknown run). Dumps below are filtered to sidebar rows, header and mode bar (the fake-claude pane text is trimmed); in the main run an injector `( sleep 6; baton debug send x/$D/alpha 'allow\r' )` ran in the background to simulate an off-screen Stop.

## status::next pure, table-driven; attention::next_after
Status: NOT PROVEN (internal — covered by tests)

## Badges (spec §4) incl. starting and `? unknown` (FAKE_CLAUDE_NO_HOOKS=1, hook_timeout_secs=1)
Status: PROVEN
```console
$ FAKE_CLAUDE_NO_HOOKS=1 baton-drive --step 'send:o' --step 'wait:1 … alpha  starting' --step dump --step 'wait:1 \? alpha  unknown' --step 'wait:2 \? beta  unknown' --step dump ... -- baton
--- screen 40x120 ---
│  1 … alpha  starting  ...
│  2 … beta  starting  ...
--- screen 40x120 ---
│  1 ? alpha  unknown  ...
│  2 ? beta  unknown  ...
$ baton debug sessions --json | jq -r '.[].status'
Unknown
Unknown
```
Starting turns into unknown after the 1 s timeout when no hook arrives. Idle (○), permission (◐), your turn (✓), exited (✗) shown below. Running (●) not separately captured. Highlight colour of attention badges not checked (text dump has no colour).

## e2e: perm -> ◐ permission; allow -> ✓ your turn off-screen; n jumps and badge becomes ○ idle; n with nothing -> hint
Status: PROVEN
```console
$ baton-drive --step 'send:o' ... (hooks on, hook_timeout_secs=20) -> after open
│  1 ○ alpha  idle
│  2 ○ beta  idle
 NORMAL │ ⏎ focus  n next-attention ...
# Enter, type "perm\r" in alpha
┌alpha ... 
│  1 ◐ alpha  permission
│  2 ○ beta  idle
 FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session
# Ctrl-\, press 2 (beta on screen), injector sends "allow\r" to alpha
│  1 ✓ alpha  your turn
│  2 ○ beta  idle
 NORMAL │ ⏎ focus  n next-attention  r restart ...
# press n
│  1 ○ alpha  idle
│  2 ○ beta  idle
 NORMAL │ ⏎ focus ...
# press n again
│  1 ○ alpha  idle
│  2 ○ beta  idle
 NORMAL │ no session needs attention
# q -> exit 0 (expect-exit:0 passed)
```
Permission shows ◐; Stop on the off-screen alpha shows ✓ while beta stays ○; `n` selects alpha (now on screen) and it becomes ○ idle; a second `n` shows the hint. (The post-n dump's header/right-pane was verified by the wait for the `1 ○ alpha  idle` row; the run's earlier dump after `n` shows alpha's pane.)

## Evidence-hint CLI: Idle and Permission via debug
Status: PROVEN
```console
$ baton debug send "x/$D/alpha" 'perm\r'; sleep 0.5; baton debug sessions --json | jq -r '.[].status'
Permission
Idle
```
(Output order alpha, beta.) Note: the first line printed by this daemon session before the send listed both sessions Idle.

## Alt-n (stays in focus mode) and Alt-1..9 in focus mode
Status: PROVEN
```console
# fresh daemon, focus mode on alpha; injector sends "perm\r" to beta
┌alpha · p · ○ idle
│  1 ○ alpha  idle
│  2 ◐ beta  permission
 FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session
# send ESC n
┌beta · p · ◐ permission
 FOCUS │ Ctrl-\ back  Alt-n next-attention  Alt-1..9 session
# send ESC 1
┌alpha · p · ○ idle
 FOCUS │ ...
# send ESC 2
┌beta · p · ◐ permission
 FOCUS │ ...
```
Alt-n jumped to the permission session and the mode bar stayed FOCUS; Alt-1 / Alt-2 switched sessions, still FOCUS.

## Child exit -> ✗ exited
Status: PROVEN
```console
# in focus mode on beta: type "exit 3\r"
┌beta · p · ✗ exited 3
│  1 ○ alpha  idle
│  2 ✗ beta  exited 3
```
Also via CLI: `baton debug sessions --json | jq -c '.[]|{id,status}'` -> beta `{"Exited":3}`.

## Client view / MarkViewed / StatusChanged
Status: NOT PROVEN (internal — covered by tests); viewed-on-screen behaviour observed indirectly (n -> ✓ became ○). Stop arriving while watching not separately exercised.

## Optional: real claude
Status: NOT PROVEN (not attempted; costs tokens, needs login)

## Cleanup
```console
$ baton daemon stop; pgrep -x baton; echo pgrep=$?
daemon stopped
pgrep=1
```
No baton or fake-claude process left; scratch dir removed; repo has no change other than this file.
