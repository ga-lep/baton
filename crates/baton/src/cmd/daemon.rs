//! `baton daemon start|stop|status`.

use crate::cli::DaemonAction;
use crate::client::{self, ClientError};
use crate::daemon::{self, Outcome};
use baton_core::paths;
use baton_proto::{ClientMsg, DaemonMsg, Role};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs a daemon subcommand.
pub fn run(action: &DaemonAction) -> ExitCode {
    let result = match action {
        DaemonAction::Start { foreground: true } => start_foreground(),
        DaemonAction::Start { foreground: false } => block_on(start()),
        DaemonAction::Stop => block_on(stop()),
        DaemonAction::Status => block_on(status()),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("baton daemon: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn block_on(fut: impl std::future::Future<Output = anyhow::Result<u8>>) -> anyhow::Result<u8> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(fut)
}

fn start_foreground() -> anyhow::Result<u8> {
    match daemon::run_foreground()? {
        Outcome::Finished => {}
        Outcome::AlreadyRunning => println!("daemon already running"),
    }
    Ok(0)
}

async fn start() -> anyhow::Result<u8> {
    match client::connect(Role::Ctl).await {
        Ok(conn) => {
            println!("daemon already running pid={}", conn.pid);
            return Ok(0);
        }
        Err(ClientError::NotRunning) => {}
        Err(e) => return Err(e.into()),
    }
    // Refuse early (before detaching) if the runtime dir is unusable.
    paths::ensure_runtime_dir()?;
    client::spawn_detached_foreground()?;
    let conn = client::connect_with_retry(Role::Ctl, client::START_TIMEOUT).await?;
    println!("daemon started pid={}", conn.pid);
    Ok(0)
}

async fn status() -> anyhow::Result<u8> {
    let mut conn = match client::connect(Role::Ctl).await {
        Ok(c) => c,
        Err(ClientError::NotRunning) => {
            println!("not running");
            return Ok(1);
        }
        Err(e) => return Err(e.into()),
    };
    conn.send(&ClientMsg::Status).await?;
    match conn.recv().await? {
        DaemonMsg::DaemonStatus {
            pid,
            version,
            sessions,
        } => {
            println!(
                "running pid={pid} protocol={version} sessions={}",
                sessions.len()
            );
            Ok(0)
        }
        other => anyhow::bail!("unexpected reply: {other:?}"),
    }
}

async fn stop() -> anyhow::Result<u8> {
    let mut conn = match client::connect(Role::Ctl).await {
        Ok(c) => c,
        Err(ClientError::NotRunning) => {
            println!("not running");
            return Ok(0);
        }
        Err(e) => return Err(e.into()),
    };
    conn.send(&ClientMsg::Shutdown).await?;
    drop(conn);
    let sock = paths::socket_path();
    let deadline = Instant::now() + STOP_TIMEOUT;
    while sock.exists() {
        if Instant::now() >= deadline {
            anyhow::bail!("daemon did not stop within {}s", STOP_TIMEOUT.as_secs());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    println!("daemon stopped");
    Ok(0)
}
