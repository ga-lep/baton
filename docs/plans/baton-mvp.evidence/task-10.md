# Evidence — Task 10: Session runtime, attach/stream, `baton debug`
Commit: c82d1cb
Environment: target/debug/baton (cargo build), scratch dir $D=/tmp/tmp.pIfJBp7VD5 with BATON_CONFIG/BATON_STATE_DIR/BATON_RUNTIME_DIR/FAKE_CLAUDE_HOME under it (removed afterwards). Config: profile p `bash --norc`, profile f = target/debug/fake-claude; project x (repos /tmp, $HOME) and project f (repo /tmp). `B=target/debug/baton`. Blank screen lines filtered with `grep -v '^\s*$'`; jq filters trimmed JSON.

## OpenProject spawns one session per repo; sessions lists 2 with status Starting
Status: PROVEN
```console
$ B daemon start
daemon started pid=991889
$ B debug open x
x//tmp
x//home/glepape
exit=0
$ B debug sessions
x//tmp  Starting
x//home/glepape  Starting
```
Two sessions are spawned, both `Starting`.

## Env (BATON_SESSION), cwd = repo, send + screen
Status: PROVEN
```console
$ B debug send 'x//tmp' 'echo marker-$BATON_SESSION; pwd\r'
exit=0
$ B debug screen 'x//tmp' | grep -v '^\s*$'
bash-5.2$ echo marker-$BATON_SESSION; pwd
marker-x//tmp
/tmp
bash-5.2$ 
```
Line `marker-x//tmp` printed; pwd is the repo path. (For the $HOME repo, only `cd; pwd` was run, which does not independently prove cwd; the /tmp session proves cwd=repo.)

## Screen survives detach/reattach
Status: PROVEN
```console
$ B debug screen 'x//tmp' | grep -v '^\s*$'     (second, separate attach)
bash-5.2$ echo marker-$BATON_SESSION; pwd
marker-x//tmp
/tmp
bash-5.2$ 
```
Each `debug screen` is its own attach/detach; the two outputs are identical.

## Scrollback
Status: PROVEN
```console
$ B debug send 'x//home/glepape' 'cd; pwd; seq 1 100\r'
$ B debug screen 'x//home/glepape' | grep -v '^\s*$' | head -3
78
79
80
$ B debug scrollback 'x//home/glepape' 0 3
bash-5.2$ cd; pwd; seq 1 100
/home/glepape
1
```
Rows scrolled off the 24-row screen are returned by GetScrollback.

## Child exit -> Exited(code), session stays listed
Status: PROVEN
```console
$ B debug send 'x//tmp' 'exit 3\r'
$ B debug sessions --json | jq -c '.[]|{id,status,exit_code}'
{"id":"x//tmp","status":{"Exited":3},"exit_code":3}
{"id":"x//home/glepape","status":"Starting","exit_code":null}
{"id":"f//tmp","status":"Starting","exit_code":null}
```
(Also repeated for x//home/glepape: `{"Exited":3}`.) Session remains listed.

## fake-claude shows `DA1: ok` with no client attached at startup
Status: PROVEN
```console
$ B debug open f; sleep 1.5; B debug screen 'f//tmp' | grep -v '^\s*$'
DA1: ok
FAKE CLAUDE session=c2017122-a384-4175-a79d-950f7cad4aae model=claude-opus-5-5
> 
```
First attach happened after startup; the daemon had answered the DA1 query itself.

## Unknown project / invalid size rejected
Status: PROVEN
```console
$ B debug open nope
baton debug: unknown project "nope"
exit=1
$ B debug screen 'x//tmp' --rows 0 --cols 80
baton debug: invalid terminal size 0x80: each side must be 1..=1000
exit=1
$ B debug screen 'x//tmp' --rows 24 --cols 60000
baton debug: invalid terminal size 24x60000: each side must be 1..=1000
exit=1
```

## Client refuses a runtime dir with mode 0755
Status: PROVEN
```console
$ BATON_RUNTIME_DIR=$D/bad B debug sessions     (chmod 755)
baton debug: i/o: refusing to use runtime dir /tmp/tmp.pIfJBp7VD5/bad: mode 755 grants group/other access (need 0700): ...
exit=1
$ BATON_RUNTIME_DIR=$D/bad B daemon status
baton daemon: i/o: refusing to use runtime dir ... mode 755 ... (need 0700)
exit=1
```

## Not exercised from outside
- Second Attach takes over with `Error{"replaced by another client"}`; attach_redraw_nudge SIGWINCH; process-group/session isolation; env BATON_SOCK; no-op resize: NOT PROVEN (internal — covered by tests; nudge/SIGWINCH covered by e2e_sessions.rs, not re-run here). The debug client is single-shot so cannot hold two clients.

## Cleanup
```console
$ B daemon stop
daemon stopped
exit=0
$ pgrep -x baton
pgrep exit=1
```
No baton process left; scratch dir removed.

EVIDENCE: PROVEN (for outside-observable criteria listed above)
