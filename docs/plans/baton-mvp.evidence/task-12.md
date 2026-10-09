# Evidence — Task 12: TUI M2 project tree, open project, switching, scrollback
Commit: 2e94676
Environment: target/debug/baton driven by target/debug/baton-drive, 40x120 PTY. Scratch dir $D (mktemp, removed afterwards) with BATON_CONFIG/BATON_STATE_DIR/BATON_RUNTIME_DIR. Profiles a/b = `bash --norc --noprofile` with env WHO=a / WHO=b. Project `loop` (default profile a, repos alpha, beta, hyper with per-repo profile b) and project `other` (repo solo). Screens trimmed: blank rows removed, long rows cut. Several drive runs shared one daemon.

## Sidebar: closed projects shown as `▸ name  (closed)`; open as `▾ name` with sessions `N <badge> <repo>  <status>`
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:▸ loop' --step dump -- baton
┌Projects──────────────────────┐┌Baton───
│▸ loop  (closed)              ││No sessions. Start a project with `baton debug open <name>`.
│▸ other  (closed)             ││
$ ... --step 'send:o' --step 'wait:3 . hyper' ... --step dump
│▾ loop                        ││
│  1 … alpha  starting         ││
│  2 … beta  starting          ││
│  3 … hyper  starting         ││
│▸ other  (closed)             ││
```
Config read on attach; closed and open rendering, and numbered sessions, as specified. (Dimming not visible in a text dump.)

## `o` opens the project under the cursor; per-repo profile env (evidence flow)
Status: PROVEN
```console
$ baton-drive ... --step 'send:o' --step 'wait:3 . hyper' --step 'send:3' --step 'send:\r' --step 'send:echo WHO=$WHO\r' --step 'wait:WHO=b' --step dump -- baton
┌Projects──────────────────────┐┌hyper · b · … starting───
│▾ loop                        ││bash-5.2$ echo WHO=$WHO
│  3 … hyper  starting         ││WHO=b
```
Session 3 (repo override profile b) prints WHO=b.

## `1`..`3` switch sessions; main panel title `<repo> · <profile> · <badge> <status>`; info panel
Status: PROVEN
```console
$ baton-drive ... (Ctrl-\ to NORMAL) --step 'send:3' --step dump --step 'send:2' --step dump ...
┌hyper · b · … starting──      │ bash-5.2$ echo WHO=$WHO / WHO=b      Session: /tmp/.../hyper, profile b, status starting, uptime 0s
┌beta · a · … starting───      │ bash-5.2$                            Session: /tmp/.../beta,  profile a, status starting, uptime 0s
$ ... Enter in session 2, echo X2=$WHO
│bash-5.2$ echo X2=$WHO / X2=a
```
Panel content and title change on switch (hyper/b shows WHO=b, beta/a shows its own content and WHO=a); info panel shows path, profile, status, uptime. In session 1 (alpha), `wait:WHO=a` also matched in an earlier run.

## Scrollback: Ctrl-u shows `[scrollback -N]` and earlier numbers; `G` returns live
Status: PROVEN
```console
$ baton-drive ... --step 'send:1' --step 'send:\r' --step 'send:clear; seq 1 500\r' --step 'wait:500 *│' --step 'send:\x1c' --step 'send:\x15' --step dump --step 'send:G' --step dump -- baton
┌alpha · a · … starting [scrollback -18]──────
│447 ... 451 (visible rows, earlier than the live tail 465..500)
 NORMAL │ ...
(after G)
┌alpha · a · … starting────────────
│bash-5.2$
```
Ctrl-u scrolled by half a page with the indicator and earlier numbers; G removed the indicator and showed the live prompt. (Live view before scrolling showed 465..500 and the prompt in an earlier run.)

## Opening the second project keeps the first project's sessions
Status: PROVEN
```console
$ baton-drive ... --step 'send:j' x5 --step 'send:o' --step 'sleep:1000' --step dump -- baton
│▾ loop                        ││
│  1 … alpha  starting         ││
│  2 … beta  starting          ││
│  3 … hyper  starting         ││
│▾ other                       ││
│  1 … solo  starting          ││
```

## Not exercised
- Alt-screen scrolling-disabled hint, `Enter`/`l` on closed project, `PgUp/PgDn/Ctrl-d`, dimming: NOT PROVEN (covered by unit tests / not visible in text dump).

## Cleanup
```console
$ baton daemon stop; pgrep -x baton; echo "pgrep rc=$?"
(daemon stop output) 
pgrep rc=1
```
