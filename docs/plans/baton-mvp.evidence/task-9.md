# Evidence — Task 9: Daemon lifecycle, socket, handshake, `baton daemon start|stop|status`
Commit: a322672
Environment: `cargo build`, target/debug/baton; BATON_CONFIG/BATON_STATE_DIR/BATON_RUNTIME_DIR in a mktemp scratch dir $D (run dir `$D/run`, state `$D/state`). Python 3 used to send raw bytes on the socket.

Raw run log (the `pgrep -af` lines in it only matched the harness's own bash wrapper, whose command line contains the word "daemon"; trimmed below to the daemon line). The real daemon process was `.../target/debug/baton daemon start --foreground`, i.e. the detached re-exec.

```console
D=/tmp/tmp.KUCyd9uFmS
$ baton daemon start
daemon started pid=936176
exit=0
$ baton daemon status
running pid=936176 protocol=1 sessions=0
exit=0
$ baton daemon start (second)
daemon already running pid=936176
exit=0
$ stat
600 /tmp/tmp.KUCyd9uFmS/run/baton.sock
700 /tmp/tmp.KUCyd9uFmS/run
600 /tmp/tmp.KUCyd9uFmS/run/baton.lock
936176 /home/glepape/project/baton/target/debug/baton daemon start --foreground
$ garbage on socket
recv b''
partial frame sent then disconnected
$ status after garbage
running pid=936176 protocol=1 sessions=0
exit=0
$ ls state; tail daemon.log
total 4
-rw-rw-r-- 1 glepape glepape 77 Oct  9 00:50 daemon.log
2026-10-08T22:50:35.656024Z  INFO baton::daemon: daemon listening pid=936176
pid=936176
$ kill -TERM pid
daemon gone
total 0
-rw------- 1 glepape glepape 0 Oct  9 00:50 baton.lock
$ status
not running
exit=1
--- restart then stop
daemon started pid=936196
running pid=936196 protocol=1 sessions=0
600
daemon stopped
stop exit=0
baton.lock
not running
exit=1
--- hint chain
daemon started pid=936204
running pid=936204 protocol=1 sessions=0
600
daemon stopped
not running
1
--- log
2026-10-08T22:50:35.747261Z  INFO baton::daemon::server: shutdown requested
2026-10-08T22:50:35.747350Z  INFO baton::daemon: daemon stopped
2026-10-08T22:50:35.778107Z  INFO baton::daemon: daemon listening pid=936204
2026-10-08T22:50:35.789818Z  INFO baton::daemon::server: shutdown requested
2026-10-08T22:50:35.789906Z  INFO baton::daemon: daemon stopped
```

## Start: detaches, prints `daemon started pid=<pid>`; `--foreground` re-exec; second start says already running, exit 0
Status: PROVEN
Output above: `daemon started pid=936176` (exit=0); process list showed `baton daemon start --foreground` as the running daemon; second start printed `daemon already running pid=936176`, exit=0. (setsid / /dev/null stdio not directly inspected; `--foreground` re-exec observed.)

## Single instance and socket (0700 dir, 0600 socket)
Status: PROVEN (flock internals and stale-socket removal not directly exercised)
`stat`: socket `600`, run dir `700`, baton.lock `600`; a second start did not spawn another daemon.

## Handshake
Status: NOT PROVEN (internal — covered by tests); garbage first frame closed the connection (`recv b''`) and daemon stayed up. Version-mismatch reply not exercised externally.

## Status
Status: PROVEN
`running pid=936176 protocol=1 sessions=0` exit=0; after stop `not running` exit=1.

## Stop
Status: PROVEN
`daemon stop` printed `daemon stopped`, exit 0, socket removed (run dir only contains baton.lock). SIGTERM to daemon pid: process gone and `ls $D/run` showed only baton.lock (socket removed); status then `not running` exit=1. SIGHUP/SIGKILL of child process groups not exercised (no sessions yet).

## Robustness and logs
Status: PROVEN
64 random bytes + 0xff*4 on the socket: connection closed, then a partial frame (length header 0x40 + 3 bytes) followed by disconnect; `status` afterward still `running pid=936176 ...`. `$D/state/daemon.log` exists with lines such as `INFO baton::daemon: daemon listening pid=936176` and `shutdown requested`.

## client::ensure_daemon()
Status: NOT PROVEN (internal — covered by tests; no CLI consumer yet)

## e2e test
Status: NOT PROVEN (not run here); equivalent flow shown manually above, including the hint chain: `daemon started pid=936204`, `running pid=936204 protocol=1 sessions=0`, `600`, `daemon stopped`, `not running`, `1`.

## Cleanup
`pgrep -x baton` printed nothing (exit 1): no daemon left running.
