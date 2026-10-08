//! IPC types and protocol version for Baton.

pub mod codec;
mod msg;

pub use codec::{ProtoError, decode, encode, framed};
pub use msg::*;

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::AsyncWriteExt;

    fn sid() -> SessionId {
        SessionId("proj/home/me/repo".into())
    }

    fn usage() -> Usage {
        Usage {
            input: 1,
            output: 2,
            cache_read: 3,
            cache_write: 4,
            context_pct: Some(12.5),
            cost_usd: Some(0.25),
            model: Some("opus".into()),
        }
    }

    fn info() -> SessionInfo {
        SessionInfo {
            id: sid(),
            project: "proj".into(),
            repo: "/home/me/repo".into(),
            profile: Some("work".into()),
            status: Status::Permission,
            claude_session_id: Some("abc".into()),
            model: Some("opus".into()),
            started_at: 1_700_000_000,
            exit_code: Some(0),
            usage: Some(usage()),
        }
    }

    fn rt<T>(v: T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let bytes = encode(&v).expect("encode");
        assert_eq!(decode::<T>(&bytes).expect("decode"), v);
    }

    #[test]
    fn client_msgs_round_trip() {
        let msgs = vec![
            ClientMsg::Hello {
                version: PROTOCOL_VERSION,
                role: Role::Tui,
            },
            ClientMsg::Hello {
                version: 1,
                role: Role::Hook,
            },
            ClientMsg::Hello {
                version: 1,
                role: Role::Ctl,
            },
            ClientMsg::Attach { rows: 24, cols: 80 },
            ClientMsg::OpenProject { name: "p".into() },
            ClientMsg::Restart { session: sid() },
            ClientMsg::Input {
                session: sid(),
                bytes: vec![0, 1, 255],
            },
            ClientMsg::Resize { rows: 10, cols: 20 },
            ClientMsg::MarkViewed { session: sid() },
            ClientMsg::ClientView {
                on_screen: Some(sid()),
                terminal_focused: true,
            },
            ClientMsg::ClientView {
                on_screen: None,
                terminal_focused: false,
            },
            ClientMsg::GetScrollback {
                session: sid(),
                start: 5,
                count: 50,
            },
            ClientMsg::Detach,
            ClientMsg::Hook {
                baton_session: sid(),
                event: "Stop".into(),
                payload_json: "{}".into(),
            },
            ClientMsg::Status,
            ClientMsg::Shutdown,
        ];
        for m in msgs {
            rt(m);
        }
    }

    #[test]
    fn daemon_msgs_round_trip() {
        let msgs = vec![
            DaemonMsg::Welcome {
                version: PROTOCOL_VERSION,
                pid: 42,
            },
            DaemonMsg::VersionMismatch { daemon_version: 9 },
            DaemonMsg::SessionList(vec![info()]),
            DaemonMsg::Snapshot {
                session: sid(),
                rows: 24,
                cols: 80,
                bytes: vec![1, 2],
            },
            DaemonMsg::Output {
                session: sid(),
                bytes: vec![3],
            },
            DaemonMsg::StatusChanged {
                session: sid(),
                status: Status::Exited(-1),
            },
            DaemonMsg::UsageUpdated {
                session: sid(),
                usage: usage(),
            },
            DaemonMsg::Scrollback {
                session: sid(),
                start: 0,
                rows: vec![b"a".to_vec()],
            },
            DaemonMsg::DaemonStatus {
                pid: 1,
                version: 2,
                sessions: vec![info()],
            },
            DaemonMsg::Error {
                message: "boom".into(),
            },
        ];
        for m in msgs {
            rt(m);
        }
    }

    #[test]
    fn data_types_round_trip() {
        for s in [
            Status::Starting,
            Status::Running,
            Status::Permission,
            Status::YourTurn,
            Status::Idle,
            Status::Exited(3),
            Status::Unknown,
        ] {
            rt(s);
        }
        rt(info());
        rt(usage());
        rt(Usage::default());
    }

    #[test]
    fn garbage_decodes_to_err() {
        assert!(decode::<ClientMsg>(&[]).is_err());
        assert!(decode::<ClientMsg>(&[0xff; 64]).is_err());
        assert!(decode::<DaemonMsg>(&[0x0a, 0xff, 0xff, 0xff]).is_err());
        // Trailing bytes after a valid value are rejected.
        let mut b = encode(&ClientMsg::Detach).expect("encode").to_vec();
        b.push(0);
        assert!(decode::<ClientMsg>(&b).is_err());
        // Every short prefix pattern must not panic.
        for i in 0..=255u8 {
            let _ = decode::<DaemonMsg>(&[i, i, i]);
            let _ = decode::<ClientMsg>(&[i]);
        }
    }

    #[test]
    fn session_id_from_canonical_path() {
        let dir = std::env::temp_dir();
        let canon = dir.canonicalize().expect("canon");
        let id = SessionId::from_repo("p", &dir).expect("id");
        assert_eq!(id.0, format!("p/{}", canon.display()));
    }

    #[tokio::test]
    async fn framed_round_trip() {
        let (a, b) = tokio::io::duplex(1 << 16);
        let mut tx = framed(a);
        let mut rx = framed(b);
        let m = ClientMsg::Input {
            session: sid(),
            bytes: vec![7; 1000],
        };
        tx.send(encode(&m).expect("encode")).await.expect("send");
        let frame = rx.next().await.expect("frame").expect("ok");
        assert_eq!(decode::<ClientMsg>(&frame).expect("decode"), m);
    }

    #[tokio::test]
    async fn oversize_frame_rejected() {
        // Receiving: a header announcing more than MAX_FRAME is an error.
        let (mut a, b) = tokio::io::duplex(1 << 16);
        let mut rx = framed(b);
        let len = u32::try_from(MAX_FRAME + 1).expect("fits");
        a.write_all(&len.to_be_bytes()).await.expect("write");
        assert!(rx.next().await.expect("item").is_err());

        // Sending: the codec refuses to emit one.
        let (c, _d) = tokio::io::duplex(64);
        let mut tx = framed(c);
        let big = bytes::Bytes::from(vec![0u8; MAX_FRAME + 1]);
        assert!(tx.send(big).await.is_err());

        // encode itself refuses oversize values.
        let huge = ClientMsg::Input {
            session: sid(),
            bytes: vec![0; MAX_FRAME + 1],
        };
        assert!(matches!(encode(&huge), Err(ProtoError::FrameTooLarge(_))));
    }
}
