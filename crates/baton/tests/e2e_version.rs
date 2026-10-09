//! End-to-end tests for `baton version [--check]` against a fake release server.

use anyhow::Result;
use baton_testkit::{ReleaseServer, Reply, bin_path};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const VER: &str = env!("CARGO_PKG_VERSION");
const URL: &str = "https://github.com/ga-lep/baton/releases/tag/v99.0.0";

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Result<Self> {
        Ok(Self {
            dir: tempfile::tempdir()?,
        })
    }

    fn cache_path(&self) -> std::path::PathBuf {
        self.dir.path().join("state/update-check.json")
    }

    fn cache(&self) -> Result<serde_json::Value> {
        Ok(serde_json::from_slice(&std::fs::read(self.cache_path())?)?)
    }

    fn run(&self, url: &str, args: &[&str], extra: &[(&str, &str)]) -> Result<Output> {
        let mut cmd = Command::new(bin_path("baton")?);
        cmd.arg("version")
            .args(args)
            .env("BATON_CONFIG", self.dir.path().join("config.toml"))
            .env("BATON_STATE_DIR", self.dir.path().join("state"))
            .env("BATON_RUNTIME_DIR", self.dir.path().join("run"))
            .env("BATON_UPDATE_URL", url)
            .stdin(Stdio::null());
        for (k, v) in extra {
            cmd.env(k, v);
        }
        Ok(cmd.output()?)
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn release(tag: &str) -> String {
    format!(r#"{{"tag_name":"{tag}","html_url":"{URL}","name":"ignored"}}"#)
}

#[test]
fn help_lists_version() -> Result<()> {
    let out = Command::new(bin_path("baton")?).arg("--help").output()?;
    assert!(stdout(&out).contains("version"));
    Ok(())
}

#[test]
fn plain_version_prints_and_never_connects() -> Result<()> {
    let env = Env::new()?;
    let srv = ReleaseServer::start(Reply::ok(release("v99.0.0")))?;
    let out = env.run(&srv.url(), &[], &[])?;
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        format!("baton {VER} (protocol {})\n", baton_proto::PROTOCOL_VERSION)
    );
    assert_eq!(srv.connections(), 0);
    Ok(())
}

#[test]
fn check_reports_available_update_and_writes_cache() -> Result<()> {
    let env = Env::new()?;
    let srv = ReleaseServer::start(Reply::ok(release("v99.0.0")))?;
    let out = env.run(&srv.url(), &["--check"], &[])?;
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        format!("baton 99.0.0 is available (you have {VER}): {URL}\n")
    );
    let cache = env.cache()?;
    assert_eq!(cache["ok"], true);
    assert_eq!(cache["latest"], "v99.0.0");
    Ok(())
}

#[test]
fn check_reports_up_to_date() -> Result<()> {
    let env = Env::new()?;
    let srv = ReleaseServer::start(Reply::ok(release(&format!("v{VER}"))))?;
    let out = env.run(&srv.url(), &["--check"], &[])?;
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), format!("baton {VER} is up to date\n"));
    Ok(())
}

#[test]
fn http_errors_exit_one_with_reason_and_failed_cache() -> Result<()> {
    for (status, reason) in [
        (404, "no public release found"),
        (403, "rate limited"),
        (429, "rate limited"),
        (500, "HTTP 500"),
    ] {
        let env = Env::new()?;
        let srv = ReleaseServer::start(Reply::status(status))?;
        let out = env.run(&srv.url(), &["--check"], &[])?;
        assert_eq!(out.status.code(), Some(1), "{status}");
        assert_eq!(
            stdout(&out),
            format!("could not check for updates: {reason}\n")
        );
        assert_eq!(env.cache()?["ok"], false);
    }
    Ok(())
}

#[test]
fn connection_refused_fails_fast() -> Result<()> {
    let env = Env::new()?;
    // A port that was just released and has no listener.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0")?;
        l.local_addr()?.port()
    };
    let t = Instant::now();
    let out = env.run(&format!("http://127.0.0.1:{port}/x"), &["--check"], &[])?;
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).starts_with("could not check for updates: "));
    assert!(t.elapsed() < Duration::from_secs(3));
    Ok(())
}

#[test]
fn hung_server_times_out() -> Result<()> {
    let env = Env::new()?;
    let srv = ReleaseServer::start(Reply::hang())?;
    let t = Instant::now();
    let out = env.run(&srv.url(), &["--check"], &[])?;
    assert_eq!(out.status.code(), Some(1));
    assert!(t.elapsed() < Duration::from_secs(4), "{:?}", t.elapsed());
    Ok(())
}

#[test]
fn request_headers_and_no_authorization() -> Result<()> {
    let env = Env::new()?;
    let srv = ReleaseServer::start(Reply::ok(release("v99.0.0")))?;
    env.run(
        &srv.url(),
        &["--check"],
        &[("GH_TOKEN", "secret"), ("GITHUB_TOKEN", "secret")],
    )?;
    let reqs = srv.requests();
    assert_eq!(reqs.len(), 1);
    let get = |k: &str| reqs[0].iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(get("user-agent"), Some(format!("baton/{VER}")));
    assert_eq!(
        get("accept").as_deref(),
        Some("application/vnd.github+json")
    );
    assert_eq!(get("x-github-api-version").as_deref(), Some("2022-11-28"));
    assert_eq!(get("authorization"), None);
    assert_eq!(get("if-none-match"), None);
    Ok(())
}

#[test]
fn etag_is_sent_and_304_keeps_latest() -> Result<()> {
    let env = Env::new()?;
    let mut reply = Reply::ok(release("v99.0.0"));
    reply.etag = Some("\"abc\"".into());
    let srv = ReleaseServer::start(reply)?;
    env.run(&srv.url(), &["--check"], &[])?;
    assert_eq!(env.cache()?["etag"], "\"abc\"");
    let first_checked = env.cache()?["checked_at"].as_u64().unwrap_or(0);

    let srv2 = ReleaseServer::start(Reply::status(304))?;
    std::thread::sleep(Duration::from_millis(1100));
    let out = env.run(&srv2.url(), &["--check"], &[])?;
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).contains("99.0.0 is available"),
        "{}",
        stdout(&out)
    );
    let sent = srv2.requests();
    assert!(
        sent[0]
            .iter()
            .any(|(k, v)| k == "if-none-match" && v == "\"abc\"")
    );
    let cache = env.cache()?;
    assert_eq!(cache["latest"], "v99.0.0");
    assert_eq!(cache["ok"], true);
    assert!(cache["checked_at"].as_u64().unwrap_or(0) > first_checked);
    Ok(())
}

#[test]
fn disabled_notice_after_explicit_check() -> Result<()> {
    let env = Env::new()?;
    let srv = ReleaseServer::start(Reply::ok(release(&format!("v{VER}"))))?;
    let out = env.run(&srv.url(), &["--check"], &[("BATON_NO_UPDATE_CHECK", "1")])?;
    assert_eq!(srv.connections(), 1);
    assert_eq!(
        stdout(&out),
        format!("baton {VER} is up to date (automatic checks are disabled)\n")
    );
    Ok(())
}

#[test]
fn oversized_body_is_rejected() -> Result<()> {
    let env = Env::new()?;
    let big = format!(
        r#"{{"tag_name":"v99.0.0","pad":"{}"}}"#,
        "x".repeat(2 << 20)
    );
    let srv = ReleaseServer::start(Reply::ok(big))?;
    let out = env.run(&srv.url(), &["--check"], &[])?;
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).starts_with("could not check for updates: "));
    Ok(())
}

#[test]
fn state_dir_and_cache_are_private_and_doctor_passes() -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let env = Env::new()?;
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0")?;
        l.local_addr()?.port()
    };
    let url = format!("http://127.0.0.1:{port}/x");
    let bin = bin_path("baton")?;
    let run = |sub: &[&str]| -> Result<Output> {
        Ok(Command::new("sh")
            .args([
                "-c",
                "umask 022; exec \"$0\" \"$@\"",
                &bin.display().to_string(),
            ])
            .args(sub)
            .env("BATON_CONFIG", env.dir.path().join("config.toml"))
            .env("BATON_STATE_DIR", env.dir.path().join("state"))
            .env("BATON_RUNTIME_DIR", env.dir.path().join("run"))
            .env("BATON_UPDATE_URL", &url)
            .stdin(Stdio::null())
            .output()?)
    };
    let out = run(&["version", "--check"])?;
    assert_eq!(out.status.code(), Some(1));
    let mode = |p: std::path::PathBuf| -> Result<u32> {
        Ok(std::fs::metadata(p)?.permissions().mode() & 0o777)
    };
    assert_eq!(mode(env.dir.path().join("state"))?, 0o700);
    assert_eq!(mode(env.cache_path())?, 0o600);
    let left: Vec<_> = std::fs::read_dir(env.dir.path().join("state"))?.collect();
    assert_eq!(left.len(), 1, "no temp files left: {left:?}");
    let doc = run(&["doctor", "--no-probe"])?;
    let text = stdout(&doc);
    assert!(text.contains("PASS dirs"), "{text}");
    assert!(!text.contains("FAIL dirs"), "{text}");
    Ok(())
}

#[test]
fn hostile_server_data_never_reaches_the_terminal() -> Result<()> {
    let env = Env::new()?;
    // Valid tag, hostile URL.
    let body =
        r#"{"tag_name":"v99.0.0","html_url":"https://github.com/\u001b]8;;https://evil\u0007x"}"#;
    let srv = ReleaseServer::start(Reply::ok(body))?;
    let out = env.run(&srv.url(), &["--check"], &[])?;
    let text = stdout(&out);
    assert!(!text.contains('\x1b') && !text.contains('\x07'), "{text:?}");
    assert!(text.contains("99.0.0 is available"), "{text:?}");
    let cached = std::fs::read_to_string(env.cache_path())?;
    assert!(!cached.contains("evil"), "{cached}");

    // Hostile tag.
    let env = Env::new()?;
    let body = r#"{"tag_name":"\u001b[2J\u001b]0;pwn\u0007","html_url":"http://x/r"}"#;
    let srv = ReleaseServer::start(Reply::ok(body))?;
    let out = env.run(&srv.url(), &["--check"], &[])?;
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1));
    assert!(!text.contains('\x1b') && !text.contains('\x07'), "{text:?}");
    Ok(())
}
