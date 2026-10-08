# Evidence — Task 7: IPC protocol crate (baton-proto)
Commit: 40a19d5
Environment: cargo workspace in repo; `cargo test -p baton-proto`; scratch consumer crate (path dep, outside repo, deleted afterwards).

## Constants and ids
Status: PROVEN
```console
PROTOCOL_VERSION=1 MAX_FRAME=16777216
id=proj//tmp          (from_repo("proj", "/tmp/../tmp") -> canonicalized)
```
MAX_FRAME is 16 MiB; SessionId is "<project>/<canonical path>".

## ClientMsg / DaemonMsg variants, Data types
Status: PROVEN (via tests; variants checked in source)
```console
$ cargo test -p baton-proto
test tests::client_msgs_round_trip ... ok
test tests::daemon_msgs_round_trip ... ok
test tests::data_types_round_trip ... ok
test tests::session_id_from_canonical_path ... ok
test result: ok. 7 passed; 0 failed
```
Round-trips cover all variants listed in msg.rs, all Status values and Usage/SessionInfo. Consumer also round-tripped Input and StatusChanged{Exited(-1)} through public API.

## Codec: encode/decode/framed
Status: PROVEN
```console
roundtrip ok=true got=Input { session: SessionId("proj//tmp"), bytes: [1, 2, 3] }
daemon rt: Ok(StatusChanged { session: SessionId("proj//tmp"), status: Exited(-1) })
```
Framed encode -> send -> recv -> decode over a duplex stream returns the original.

## Tests: garbage returns Err, oversize rejected
Status: PROVEN
```console
garbage: true
encode oversize: Err(FrameTooLarge(16777224))
oversize header recv: Err(Custom { kind: InvalidData, error: LengthDelimitedCodecError })
test tests::garbage_decodes_to_err ... ok
test tests::oversize_frame_rejected ... ok
```
Garbage gives Err without panic; over-MAX_FRAME is rejected on encode and on the framed receive path.
