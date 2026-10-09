//! Hidden `baton debug ...` client: an evidence surface that needs no TUI.

use crate::client::{self, Conn};
use crate::term::screen::Screen;
use crate::term::vt100_screen::Vt100Screen;
use anyhow::{Context, Result, bail};
use baton_proto::{ClientMsg, DaemonMsg, Role, SessionId, SessionInfo};
use clap::{Parser, Subcommand};
use std::process::ExitCode;
use std::time::Duration;
use tokio::time::Instant;

/// How long `screen` keeps streaming output after the snapshot.
const SETTLE: Duration = Duration::from_millis(300);
/// How long to wait for the daemon to answer.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Parser)]
#[command(name = "baton debug", no_binary_name = true)]
struct DebugCli {
    #[command(subcommand)]
    cmd: Sub,
}

#[derive(Debug, Subcommand)]
enum Sub {
    /// Start the sessions of a configured project.
    Open { project: String },
    /// List sessions.
    Sessions {
        #[arg(long)]
        json: bool,
    },
    /// Type text into a session (escapes: \r \n \t \e \\ \xNN).
    Send { session: String, text: String },
    /// Kill a session's child and launch it again, resuming its conversation.
    Restart { session: String },
    /// Attach and print the session screen as text.
    Screen {
        session: String,
        #[arg(long, default_value_t = 24)]
        rows: u16,
        #[arg(long, default_value_t = 80)]
        cols: u16,
    },
    /// Print scrollback rows.
    Scrollback {
        session: String,
        start: u32,
        count: u32,
    },
}

/// Runs `baton debug <args>`.
pub fn run(args: &[String]) -> ExitCode {
    let cli = match DebugCli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(2);
        }
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("baton debug: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(dispatch(cli.cmd)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("baton debug: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(cmd: Sub) -> Result<()> {
    match cmd {
        Sub::Open { project } => open(&project).await,
        Sub::Sessions { json } => sessions(json).await,
        Sub::Send { session, text } => send(&session, &text).await,
        Sub::Restart { session } => restart(&session).await,
        Sub::Screen {
            session,
            rows,
            cols,
        } => screen(&session, rows, cols).await,
        Sub::Scrollback {
            session,
            start,
            count,
        } => scrollback(&session, start, count).await,
    }
}

async fn recv(conn: &mut Conn) -> Result<DaemonMsg> {
    match tokio::time::timeout(REPLY_TIMEOUT, conn.recv()).await {
        Ok(msg) => Ok(msg?),
        Err(_) => bail!("timed out waiting for the daemon"),
    }
}

/// Receives until `pick` accepts a message; a daemon `Error` becomes an error.
async fn recv_until<T>(conn: &mut Conn, mut pick: impl FnMut(DaemonMsg) -> Option<T>) -> Result<T> {
    loop {
        match recv(conn).await? {
            DaemonMsg::Error { message } => bail!("{message}"),
            other => {
                if let Some(v) = pick(other) {
                    return Ok(v);
                }
            }
        }
    }
}

async fn open(project: &str) -> Result<()> {
    let mut conn = client::ensure_daemon(Role::Ctl).await?;
    conn.send(&ClientMsg::OpenProject {
        name: project.to_owned(),
    })
    .await?;
    let list = recv_until(&mut conn, |m| match m {
        DaemonMsg::SessionList(l) => Some(l),
        _ => None,
    })
    .await?;
    for s in list {
        println!("{}", s.id);
    }
    Ok(())
}

async fn fetch_sessions(conn: &mut Conn) -> Result<Vec<SessionInfo>> {
    conn.send(&ClientMsg::Status).await?;
    recv_until(conn, |m| match m {
        DaemonMsg::DaemonStatus { sessions, .. } => Some(sessions),
        _ => None,
    })
    .await
}

async fn sessions(json: bool) -> Result<()> {
    let mut conn = client::ensure_daemon(Role::Ctl).await?;
    let list = fetch_sessions(&mut conn).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&list)?);
    } else {
        for s in &list {
            println!("{}  {:?}", s.id, s.status);
        }
    }
    Ok(())
}

async fn send(session: &str, text: &str) -> Result<()> {
    let mut conn = client::ensure_daemon(Role::Ctl).await?;
    conn.send(&ClientMsg::Input {
        session: SessionId(session.to_owned()),
        bytes: unescape(text)?,
    })
    .await?;
    // A Status round trip surfaces an error reply to the Input, if any.
    fetch_sessions(&mut conn).await?;
    Ok(())
}

async fn restart(session: &str) -> Result<()> {
    let mut conn = client::ensure_daemon(Role::Ctl).await?;
    conn.send(&ClientMsg::Restart {
        session: SessionId(session.to_owned()),
    })
    .await?;
    // A Status round trip surfaces an error reply to the Restart, if any.
    fetch_sessions(&mut conn).await?;
    Ok(())
}

async fn screen(session: &str, rows: u16, cols: u16) -> Result<()> {
    let target = SessionId(session.to_owned());
    let mut conn = client::ensure_daemon(Role::Tui).await?;
    conn.send(&ClientMsg::Attach { rows, cols }).await?;
    let mut mirror: Option<Vt100Screen> = None;
    let mut deadline = Instant::now() + REPLY_TIMEOUT;
    let mut known = false;
    loop {
        let msg = match tokio::time::timeout_at(deadline, conn.recv()).await {
            Ok(m) => m?,
            Err(_) if mirror.is_some() => break, // settled
            Err(_) => bail!("timed out waiting for the snapshot of {session}"),
        };
        match msg {
            DaemonMsg::Error { message } => bail!("{message}"),
            DaemonMsg::SessionList(list) => {
                known = list.iter().any(|s| s.id == target);
                if !known {
                    bail!("unknown session \"{session}\"");
                }
            }
            DaemonMsg::Snapshot {
                session,
                rows,
                cols,
                bytes,
            } if known && session == target => {
                // Replies to terminal queries are dropped: the daemon answers.
                let mut m = Vt100Screen::new(rows, cols, 0);
                m.process(&bytes);
                mirror = Some(m);
                deadline = Instant::now() + SETTLE;
            }
            DaemonMsg::Output { session, bytes } if session == target => {
                if let Some(m) = mirror.as_mut() {
                    m.process(&bytes);
                }
            }
            _ => {}
        }
    }
    let m = mirror.context("no snapshot received")?;
    println!("{}", m.contents());
    Ok(())
}

async fn scrollback(session: &str, start: u32, count: u32) -> Result<()> {
    let mut conn = client::ensure_daemon(Role::Ctl).await?;
    conn.send(&ClientMsg::GetScrollback {
        session: SessionId(session.to_owned()),
        start,
        count,
    })
    .await?;
    let rows = recv_until(&mut conn, |m| match m {
        DaemonMsg::Scrollback { rows, .. } => Some(rows),
        _ => None,
    })
    .await?;
    for row in rows {
        println!("{}", String::from_utf8_lossy(&row).replace('\u{1b}', "\\e"));
    }
    Ok(())
}

/// Unescapes `\r \n \t \e \\ \xNN` (the same escapes as `baton-drive`).
fn unescape(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut it = s.chars();
    while let Some(ch) = it.next() {
        if ch != '\\' {
            let mut b = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
            continue;
        }
        match it.next() {
            Some('r') => out.push(b'\r'),
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('e') => out.push(0x1b),
            Some('\\') => out.push(b'\\'),
            Some('x') => {
                let hex: String = it.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&hex, 16).with_context(|| format!("bad \\x{hex}"))?);
            }
            Some(other) => bail!("unknown escape \\{other}"),
            None => bail!("trailing backslash"),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_like_baton_drive() {
        assert_eq!(
            unescape("a\\r\\n\\t\\e\\\\\\x41é").unwrap(),
            b"a\r\n\t\x1b\\A\xc3\xa9"
        );
        assert!(unescape("\\q").is_err());
        assert!(unescape("x\\").is_err());
        assert!(unescape("\\xZZ").is_err());
    }

    #[test]
    fn subcommands_parse() {
        let p = |a: &[&str]| DebugCli::try_parse_from(a.iter().copied());
        assert!(p(&["open", "x"]).is_ok());
        assert!(p(&["sessions", "--json"]).is_ok());
        assert!(p(&["send", "x//tmp", "hi\\r"]).is_ok());
        assert!(p(&["restart", "x//tmp"]).is_ok());
        assert!(p(&["screen", "x//tmp", "--rows", "30", "--cols", "100"]).is_ok());
        assert!(p(&["scrollback", "x//tmp", "0", "10"]).is_ok());
        assert!(p(&["bogus"]).is_err());
    }
}
