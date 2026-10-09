//! End-to-end tests for `baton doctor`.

use anyhow::Result;
use baton_testkit::{ReleaseServer, Reply, bin_path, scrub_env};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

/// Temp environment with one `fake-claude` profile `p`.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new(profile_env: &str) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("a"))?;
        std::fs::create_dir(dir.path().join("tmp"))?;
        let root = dir.path().canonicalize()?;
        let config = format!(
            "[profiles.p]\ncommand = {cmd:?}\n[profiles.p.env]\n{profile_env}\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\n\
             repos = [{{ path = \"{r}/a\" }}]\n",
            cmd = bin_path("fake-claude")?.display().to_string(),
            r = root.display()
        );
        std::fs::write(dir.path().join("config.toml"), config)?;
        Ok(Self { dir })
    }

    fn doctor(&self, args: &[&str]) -> Result<Output> {
        self.doctor_with(args, &[])
    }

    /// Like `doctor`, with extra environment applied after the defaults
    /// (which disable the update check).
    fn doctor_with(&self, args: &[&str], extra: &[(&str, &str)]) -> Result<Output> {
        let mut cmd = Command::new(bin_path("baton")?);
        scrub_env(&mut cmd);
        cmd.arg("doctor")
            .args(args)
            .env("BATON_NO_UPDATE_CHECK", "1")
            .env("BATON_CONFIG", self.dir.path().join("config.toml"))
            .env("BATON_STATE_DIR", self.dir.path().join("state"))
            .env("BATON_RUNTIME_DIR", self.dir.path().join("run"))
            .env("BATON_NOTIFY_SINK", "off")
            .env("BATON_DOCTOR_PROBE_TIMEOUT_SECS", "4")
            .env("FAKE_CLAUDE_HOME", self.dir.path().join("fake-claude"))
            .env("TMPDIR", self.dir.path().join("tmp"))
            .stdin(Stdio::null());
        for (k, v) in extra {
            cmd.env(k, v);
        }
        Ok(cmd.output()?)
    }

    /// Writes `state/update-check.json`.
    fn write_cache(&self, checked_ago: u64, ok: bool, latest: Option<&str>) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let json = serde_json::json!({
            "checked_at": now - checked_ago,
            "latest": latest,
            "html_url": latest.map(|_| "https://github.com/ga-lep/baton/releases/tag/v99.0.0"),
            "etag": null,
            "ok": ok,
        });
        let state = self.dir.path().join("state");
        std::fs::create_dir_all(&state)?;
        std::fs::set_permissions(&state, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
        let file = self.dir.path().join("state/update-check.json");
        std::fs::write(&file, serde_json::to_vec(&json)?)?;
        std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        Ok(())
    }

    /// The probe's private temp dir must be gone afterwards.
    fn assert_tmp_clean(&self) -> Result<()> {
        let left: Vec<_> = std::fs::read_dir(self.dir.path().join("tmp"))?.collect();
        assert!(left.is_empty(), "probe temp dir left behind: {left:?}");
        Ok(())
    }
}

#[test]
fn healthy_fake_claude_profile_passes_every_check() -> Result<()> {
    let env = Env::new("")?;
    let out = env.doctor(&[])?;
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("PASS hooks: SessionStart received (profile p)"),
        "{text}"
    );
    assert!(text.contains("PASS config"), "{text}");
    assert!(text.contains("PASS dirs"), "{text}");
    assert!(text.contains("WARN daemon"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
    env.assert_tmp_clean()
}

#[test]
fn hooks_that_never_fire_fail_the_probe_with_a_hint() -> Result<()> {
    let env = Env::new("FAKE_CLAUDE_NO_HOOKS = \"1\"")?;
    let out = env.doctor(&[])?;
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("FAIL hooks"), "{text}");
    assert!(
        text.contains("trust dialog pending or hooks disabled"),
        "{text}"
    );
    env.assert_tmp_clean()
}

#[test]
fn no_probe_skips_the_hook_check() -> Result<()> {
    let env = Env::new("FAKE_CLAUDE_NO_HOOKS = \"1\"")?;
    let out = env.doctor(&["--no-probe"])?;
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(!text.contains("hooks"), "{text}");
    Ok(())
}

#[test]
fn invalid_config_fails() -> Result<()> {
    let env = Env::new("")?;
    std::fs::write(env.dir.path().join("config.toml"), "not [valid")?;
    let out = env.doctor(&[])?;
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("FAIL config"), "{text}");
    Ok(())
}

const VER: &str = env!("CARGO_PKG_VERSION");

/// The `version:` lines of a doctor run.
fn version_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains(" version: ") && !l.contains("(profile "))
        .map(str::to_owned)
        .collect()
}

#[test]
fn version_line_reports_disabled_check_without_connecting() -> Result<()> {
    let env = Env::new("")?;
    let srv = ReleaseServer::start(Reply::ok(r#"{"tag_name":"v99.0.0"}"#))?;
    let out = env.doctor_with(&["--no-probe"], &[("BATON_UPDATE_URL", &srv.url())])?;
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        version_lines(&out),
        [format!("PASS version: {VER} (update check disabled)")]
    );
    assert_eq!(srv.connections(), 0);
    Ok(())
}

#[test]
fn version_line_respects_config_key() -> Result<()> {
    let env = Env::new("")?;
    let cfg = env.dir.path().join("config.toml");
    let text = std::fs::read_to_string(&cfg)?;
    std::fs::write(&cfg, format!("update_check = false\n{text}"))?;
    let srv = ReleaseServer::start(Reply::ok(r#"{"tag_name":"v99.0.0"}"#))?;
    let out = env.doctor_with(
        &["--no-probe"],
        &[
            ("BATON_NO_UPDATE_CHECK", ""),
            ("BATON_UPDATE_URL", &srv.url()),
        ],
    )?;
    assert_eq!(
        version_lines(&out),
        [format!("PASS version: {VER} (update check disabled)")]
    );
    assert_eq!(srv.connections(), 0);
    Ok(())
}

#[test]
fn fresh_cache_with_newer_release_warns_without_connecting() -> Result<()> {
    let env = Env::new("")?;
    env.write_cache(60, true, Some("v99.0.0"))?;
    let srv = ReleaseServer::start(Reply::ok(r#"{"tag_name":"v1.0.0"}"#))?;
    let out = env.doctor_with(
        &["--no-probe"],
        &[
            ("BATON_NO_UPDATE_CHECK", ""),
            ("BATON_UPDATE_URL", &srv.url()),
        ],
    )?;
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        version_lines(&out),
        [format!(
            "WARN version: 99.0.0 is available (you have {VER}): https://github.com/ga-lep/baton/releases/tag/v99.0.0"
        )]
    );
    assert_eq!(srv.connections(), 0);
    Ok(())
}

#[test]
fn newer_without_url_has_no_dangling_separator() -> Result<()> {
    let env = Env::new("")?;
    env.write_cache(60, true, Some("v99.0.0"))?;
    let file = env.dir.path().join("state/update-check.json");
    let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
    json["html_url"] = serde_json::Value::Null;
    std::fs::write(&file, serde_json::to_vec(&json)?)?;
    let out = env.doctor_with(&["--no-probe"], &[("BATON_NO_UPDATE_CHECK", "")])?;
    assert_eq!(
        version_lines(&out),
        [format!(
            "WARN version: 99.0.0 is available (you have {VER})"
        )]
    );
    Ok(())
}

#[test]
fn fresh_failed_cache_with_known_release_still_warns_last_check_failed() -> Result<()> {
    let env = Env::new("")?;
    env.write_cache(60, false, Some("v99.0.0"))?;
    let srv = ReleaseServer::start(Reply::ok(r#"{"tag_name":"v99.0.0"}"#))?;
    let out = env.doctor_with(
        &["--no-probe"],
        &[
            ("BATON_NO_UPDATE_CHECK", ""),
            ("BATON_UPDATE_URL", &srv.url()),
        ],
    )?;
    assert_eq!(
        version_lines(&out),
        ["WARN version: could not check for updates (last check failed)".to_owned()]
    );
    assert_eq!(srv.connections(), 0);
    Ok(())
}

#[test]
fn fresh_cache_up_to_date_passes() -> Result<()> {
    let env = Env::new("")?;
    env.write_cache(60, true, Some(&format!("v{VER}")))?;
    let out = env.doctor_with(&["--no-probe"], &[("BATON_NO_UPDATE_CHECK", "")])?;
    assert_eq!(
        version_lines(&out),
        [format!("PASS version: {VER} (latest)")]
    );
    Ok(())
}

#[test]
fn stale_cache_makes_one_connection_and_rewrites_cache() -> Result<()> {
    let env = Env::new("")?;
    env.write_cache(48 * 3600, true, Some("v1.0.0"))?;
    let srv = ReleaseServer::start(Reply::ok(
        r#"{"tag_name":"v98.0.0","html_url":"https://github.com/ga-lep/baton/releases/tag/v98.0.0"}"#,
    ))?;
    let out = env.doctor_with(
        &["--no-probe"],
        &[
            ("BATON_NO_UPDATE_CHECK", ""),
            ("BATON_UPDATE_URL", &srv.url()),
        ],
    )?;
    assert_eq!(
        version_lines(&out),
        [format!(
            "WARN version: 98.0.0 is available (you have {VER}): https://github.com/ga-lep/baton/releases/tag/v98.0.0"
        )]
    );
    assert_eq!(srv.connections(), 1);
    let cache: serde_json::Value = serde_json::from_slice(&std::fs::read(
        env.dir.path().join("state/update-check.json"),
    )?)?;
    assert_eq!(cache["latest"], "v98.0.0");
    Ok(())
}

#[test]
fn fresh_failed_cache_warns_without_connecting() -> Result<()> {
    let env = Env::new("")?;
    env.write_cache(60, false, None)?;
    let srv = ReleaseServer::start(Reply::ok(r#"{"tag_name":"v99.0.0"}"#))?;
    let out = env.doctor_with(
        &["--no-probe"],
        &[
            ("BATON_NO_UPDATE_CHECK", ""),
            ("BATON_UPDATE_URL", &srv.url()),
        ],
    )?;
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        version_lines(&out),
        ["WARN version: could not check for updates (last check failed)".to_owned()]
    );
    assert_eq!(srv.connections(), 0);
    Ok(())
}

#[test]
fn unreachable_server_warns_and_keeps_exit_zero() -> Result<()> {
    let env = Env::new("")?;
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0")?;
        l.local_addr()?.port()
    };
    let url = format!("http://127.0.0.1:{port}/x");
    let started = std::time::Instant::now();
    let out = env.doctor_with(
        &["--no-probe"],
        &[("BATON_NO_UPDATE_CHECK", ""), ("BATON_UPDATE_URL", &url)],
    )?;
    assert!(started.elapsed() < Duration::from_secs(8));
    assert_eq!(out.status.code(), Some(0));
    let lines = version_lines(&out);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("WARN version: could not check for updates ("),
        "{lines:?}"
    );
    Ok(())
}

#[test]
fn version_line_follows_notify_line() -> Result<()> {
    let env = Env::new("")?;
    let out = env.doctor(&["--no-probe"])?;
    let text = String::from_utf8_lossy(&out.stdout);
    let notify = text.find(" notify: ").expect("notify line");
    let version = text.find("version: 0").expect("version line");
    assert!(notify < version, "{text}");
    Ok(())
}

#[test]
fn planted_cache_in_untrusted_state_dir_is_ignored() -> Result<()> {
    for (ok, latest) in [(true, Some("v99.0.0")), (false, None)] {
        let env = Env::new("")?;
        env.write_cache(60, ok, latest)?;
        std::fs::set_permissions(
            env.dir.path().join("state"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )?;
        let srv = ReleaseServer::start(Reply::ok(r#"{"tag_name":"v1.0.0"}"#))?;
        let out = env.doctor_with(
            &["--no-probe"],
            &[
                ("BATON_NO_UPDATE_CHECK", ""),
                ("BATON_UPDATE_URL", &srv.url()),
            ],
        )?;
        let lines = version_lines(&out);
        assert_eq!(srv.connections(), 1, "{lines:?}");
        assert!(
            lines
                .iter()
                .all(|l| !l.contains("99.0.0") && !l.contains("last check failed")),
            "{lines:?}"
        );
    }
    Ok(())
}
