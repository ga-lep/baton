# Evidence — Task 3: baton version [--check] (ureq fetcher)
Commit: d327f04
Environment: debug build `target/debug/baton`; throwaway python socket server (scratch dir outside repo, ports 8101-8110) driven via BATON_UPDATE_URL=http://127.0.0.1:<port>/ and BATON_STATE_DIR in scratch. Servers stopped afterwards. Real API run from /tmp.

## `baton --help` lists `version`; `baton version` prints version line, exit 0, no connection
Status: PROVEN
```console
$ baton --help | grep -n version
11:  version     Print the version; with `--check`, look for a newer release
$ BATON_UPDATE_URL=http://127.0.0.1:8101/ baton version; echo exit=$?
baton 0.1.0 (protocol 6)
exit=0
$ ls <scratch>/log8101      # server request log
ls: cannot access '.../log8101': No such file or directory
```
Server logs each request to that file; the file never got created, so 0 connections.

## Real API (repo public, no release)
Status: PROVEN
```console
$ BATON_STATE_DIR=$(mktemp -d) baton version --check; echo exit=$?
could not check for updates: no public release found
exit=1
```

## --check with newer tag: message, exit 0, cache ok:true; headers; no Authorization
Status: PROVEN
```console
$ GH_TOKEN=*** GITHUB_TOKEN=*** baton version --check; echo exit=$?
baton 99.0.0 is available (you have 0.1.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0
exit=0
$ cat $BATON_STATE_DIR/update-check.json
{"checked_at":1791553100,"latest":"v99.0.0","html_url":"https://github.com/ga-lep/baton/releases/tag/v99.0.0","etag":"\"abc123\"","ok":true}
--- request seen by server:
GET / HTTP/1.1
host: 127.0.0.1:8102
accept: application/vnd.github+json
x-github-api-version: 2022-11-28
user-agent: baton/0.1.0
```
User-Agent, Accept, API-version present; no authorization header although both token vars were set.

## Non-github html_url is dropped
Status: PROVEN (cosmetic note below)
```console
$ baton version --check      # html_url https://evil.example/x
baton 99.0.0 is available (you have 0.1.0): 
exit=0
{"checked_at":1791553102,"latest":"v99.0.0","html_url":null,"etag":"\"abc123\"","ok":true}
```
URL not printed and cached as null. Observation (not a criterion): output keeps a dangling ": " with trailing space when the URL is dropped.

## tag_name == current version
Status: PROVEN
```console
$ baton version --check; echo exit=$?
baton 0.1.0 is up to date
exit=0
```

## Error cases
Status: PROVEN
```console
404: could not check for updates: no public release found   exit=1
     cache: {"checked_at":1791553103,"latest":null,"html_url":null,"etag":null,"ok":false}
403: could not check for updates: rate limited              exit=1   (cache ok:false)
429: could not check for updates: rate limited              exit=1   (cache ok:false)
refused (closed port 8199):
could not check for updates: io: Connection refused (os error 111)
exit=1      real 0m0.009s
accept-but-never-answer:
could not check for updates: timeout: global
exit=1      real 0m3.066s
```

## ETag / If-None-Match and 304
Status: PROVEN
```console
# 2nd request with cached ETag, server saw:
GET / HTTP/1.1 ... if-none-match: "abc123"
# server then answered 304 (1 s later):
$ baton version --check; echo exit=$?
baton 99.0.0 is available (you have 0.1.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0
exit=0
before: {"checked_at":1791553100,"latest":"v99.0.0",...,"etag":"\"abc123\"","ok":true}
after:  {"checked_at":1791553102,"latest":"v99.0.0",...,"etag":"\"abc123\"","ok":true}
```
`latest` kept, `checked_at` advanced; request carried If-None-Match.

## BATON_NO_UPDATE_CHECK=1 still queries and annotates
Status: PROVEN
```console
$ BATON_NO_UPDATE_CHECK=1 baton version --check; echo exit=$?
baton 99.0.0 is available (you have 0.1.0): https://github.com/ga-lep/baton/releases/tag/v99.0.0 (automatic checks are disabled)
exit=0
server requests: 1
```

## Bodies > 1 MiB rejected without reading fully
Status: PROVEN
```console
$ baton version --check      # server advertises/sends 5 MiB
could not check for updates: response too large
exit=1      real 0m0.023s
server log: SENT 3604480 stopped: [Errno 104] Connection reset by peer
```
Client aborted after ~3.4 MiB of 5 MiB had been pushed into socket buffers (server could not finish sending).

## No OpenSSL
Status: PROVEN
```console
$ cargo tree -p baton -i openssl-sys
error: package ID specification `openssl-sys` did not match any packages
exit=101
```
The package is not in the dependency graph at all.

## Commit message records stripped release size
Status: PROVEN
```console
$ git log 6f55f56 -1
Stripped release size of baton: before 6262096 B, after 8344616 B (+2082520 B, ~2.0 MB).
```
(Recorded in commit 6f55f56 in the range; I did not rebuild a release binary to verify the numbers.)

## Fix: private cache perms (umask 022, fresh dir, unreachable URL)
Status: PROVEN
```console
$ (umask 022; BATON_STATE_DIR=$S/fresh/st BATON_UPDATE_URL=http://127.0.0.1:8199/ baton version --check; echo exit=$?)
could not check for updates: io: Connection refused (os error 111)
exit=1
$ stat -c '%a %n' $S/fresh/st $S/fresh/st/update-check.json
700 .../fresh/st
600 .../fresh/st/update-check.json
$ BATON_STATE_DIR=$S/fresh/st baton doctor --no-probe | grep -i state
PASS dirs: state dir .../fresh/st is writable
```

## Fix: terminal escapes sanitised
Status: PROVEN
Server returned html_url `https://github.com/ga-lep/baton/\u001b[31m\u0007x` (valid tag).
```console
$ baton version --check | od -c
0000000   b   a   t   o   n       9   9   .   0   .   0       i   s    
0000020   a   v   a   i   l   a   b   l   e       (   y   o   u       h
0000040   a   v   e       0   .   1   .   0   )   :      \n
0000055
esc/bel bytes stdout: 0  stderr: 0
```
Also, with ESC/BEL inside tag_name (`v99.0.0\u001b[31m\u0007x`) the output was `unrecognised release tag "v99.0.0?[31m?x": unexpected character '\u{1b}' after patch...` with 0 ESC/BEL bytes (escapes replaced by `?`). The unsafe URL is dropped (cache html_url null).

## Verdict
EVIDENCE: PROVEN
