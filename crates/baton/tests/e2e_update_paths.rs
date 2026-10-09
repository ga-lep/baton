//! The hook, statusline, daemon and config paths never reach the update
//! check; the TUI shows a non-blocking notice.

use anyhow::Result;
use baton_testkit::{Drive, DriveOptions, ReleaseServer, Reply, bin_path};
use regex::Regex;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(15);

struct Env {
    dir: tempfile::TempDir,
    url: String,
}

impl Env {
    fn new(url: String) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        std::fs::create_dir(root.join("a"))?;
        let config = format!(
            "[profiles.p]\ncommand = \"bash --norc --noprofile -s --\"\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\n\
             repos = [{{ path = \"{r}/a\" }}]\n",
            r = root.display()
        );
        std::fs::write(dir.path().join("config.toml"), config)?;
        Ok(Self { dir, url })
    }

    fn envs(&self) -> Vec<(String, String)> {
        let p = |n: &str| self.dir.path().join(n).display().to_string();
        vec![
            ("BATON_CONFIG".into(), p("config.toml")),
            ("BATON_STATE_DIR".into(), p("state")),
            ("BATON_RUNTIME_DIR".into(), p("run")),
            ("BATON_NOTIFY_SINK".into(), "off".into()),
            ("BATON_UPDATE_URL".into(), self.url.clone()),
        ]
    }

    fn cache_path(&self) -> std::path::PathBuf {
        self.dir.path().join("state/update-check.json")
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<Output> {
        let mut c = Command::new(bin_path("baton")?);
        c.args(args)
            .env_remove("BATON_SESSION")
            .env_remove("BATON_SOCK")
            .env_remove("BATON_NO_UPDATE_CHECK")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in self.envs() {
            c.env(k, v);
        }
        let mut child = c.spawn()?;
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(stdin.unwrap_or("").as_bytes());
        }
        Ok(child.wait_with_output()?)
    }

    fn tui(&self) -> Result<Drive> {
        let argv = vec![bin_path("baton")?.display().to_string()];
        let env = self
            .envs()
            .into_iter()
            .chain([("BATON_NO_UPDATE_CHECK".to_owned(), "0".to_owned())])
            .collect();
        Drive::spawn(
            &argv,
            &DriveOptions {
                rows: 30,
                cols: 120,
                env,
                cwd: None,
            },
        )
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.run(&["daemon", "stop"], None);
    }
}

fn wait(d: &Drive, pattern: &str) -> Result<()> {
    d.wait_for(&Regex::new(pattern)?, WAIT)
}

fn release(tag: &str) -> String {
    format!(r#"{{"tag_name":"{tag}","html_url":"https://example.invalid/r"}}"#)
}

#[test]
fn hook_statusline_daemon_config_and_version_never_connect() -> Result<()> {
    let server = ReleaseServer::start(Reply::ok(release("v99.0.0")))?;
    let env = Env::new(server.url())?;

    let started = Instant::now();
    let hook = env.run(&["hook", "Stop"], Some("{}"))?;
    assert!(started.elapsed() < Duration::from_secs(1), "hook too slow");
    assert!(hook.status.success());
    assert!(hook.stdout.is_empty() && hook.stderr.is_empty());

    let line = env.run(&["statusline"], Some("{}"))?;
    assert!(line.status.success());
    assert!(line.stdout.is_empty() && line.stderr.is_empty());

    assert!(env.run(&["daemon", "start"], None)?.status.success());
    assert!(env.run(&["daemon", "stop"], None)?.status.success());
    assert!(env.run(&["config", "check"], None)?.status.success());
    assert!(env.run(&["version"], None)?.status.success());

    assert_eq!(server.connections(), 0);
    assert!(!env.cache_path().exists());
    Ok(())
}

#[test]
fn tui_shows_notice_from_fetch_and_writes_cache() -> Result<()> {
    let server = ReleaseServer::start(Reply::ok(release("v99.0.0")))?;
    let env = Env::new(server.url())?;
    let mut d = env.tui()?;
    wait(&d, "v99.0.0 available")?;
    assert!(env.cache_path().exists());
    d.send(b"q")?;
    assert_eq!(d.wait_exit(Duration::from_secs(5))?, 0);
    Ok(())
}

#[test]
fn tui_shows_notice_from_fresh_cache_without_connecting() -> Result<()> {
    let server = ReleaseServer::start(Reply::status(500))?;
    let env = Env::new(server.url())?;
    std::fs::create_dir_all(env.dir.path().join("state"))?;
    std::fs::set_permissions(
        env.dir.path().join("state"),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    std::fs::write(
        env.cache_path(),
        format!(
            r#"{{"checked_at":{now},"latest":"v99.0.0","html_url":null,"etag":null,"ok":true}}"#
        ),
    )?;
    std::fs::set_permissions(
        env.cache_path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )?;
    let mut d = env.tui()?;
    wait(&d, "v99.0.0 available")?;
    d.send(b"q")?;
    assert_eq!(d.wait_exit(Duration::from_secs(5))?, 0);
    assert_eq!(server.connections(), 0);
    Ok(())
}

#[test]
fn tui_is_silent_on_404() -> Result<()> {
    let server = ReleaseServer::start(Reply::status(404))?;
    let env = Env::new(server.url())?;
    let mut d = env.tui()?;
    wait(&d, "Projects")?;
    let cache = std::fs::read(env.cache_path());
    let started = Instant::now();
    let cache = loop {
        if let Ok(c) = std::fs::read(env.cache_path()) {
            break c;
        }
        assert!(started.elapsed() < WAIT, "cache never written: {cache:?}");
        std::thread::sleep(Duration::from_millis(50));
    };
    let v: serde_json::Value = serde_json::from_slice(&cache)?;
    assert_eq!(v["ok"], false);
    let screen = d.screen_text().to_lowercase();
    assert!(!screen.contains("available"), "{screen}");
    assert!(!screen.contains("error"), "{screen}");
    assert!(!screen.contains("404"), "{screen}");
    d.send(b"q")?;
    assert_eq!(d.wait_exit(Duration::from_secs(5))?, 0);
    Ok(())
}

#[test]
fn tui_does_not_wait_for_a_hung_server() -> Result<()> {
    let server = ReleaseServer::start(Reply::hang())?;
    let env = Env::new(server.url())?;
    let mut d = env.tui()?;
    wait(&d, "Projects")?;
    let started = Instant::now();
    while server.connections() == 0 {
        assert!(started.elapsed() < WAIT, "no fetch started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let screen = d.screen_text().to_lowercase();
    assert!(!screen.contains("error") && !screen.contains("available"));
    let quit = Instant::now();
    d.send(b"q")?;
    assert_eq!(d.wait_exit(Duration::from_secs(1))?, 0);
    assert!(quit.elapsed() < Duration::from_secs(1));
    Ok(())
}

#[test]
fn disabled_tui_does_not_connect() -> Result<()> {
    let server = ReleaseServer::start(Reply::ok(release("v99.0.0")))?;
    let env = Env::new(server.url())?;
    let argv = vec![bin_path("baton")?.display().to_string()];
    let mut d = Drive::spawn(
        &argv,
        &DriveOptions {
            rows: 30,
            cols: 120,
            env: env
                .envs()
                .into_iter()
                .chain([("BATON_NO_UPDATE_CHECK".to_owned(), "1".to_owned())])
                .collect(),
            cwd: None,
        },
    )?;
    wait(&d, "Projects")?;
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(server.connections(), 0);
    assert!(!env.cache_path().exists());
    d.send(b"q")?;
    d.wait_exit(Duration::from_secs(5))?;
    Ok(())
}

#[test]
fn tui_ignores_group_writable_cache_file() -> Result<()> {
    let server = ReleaseServer::start(Reply::status(500))?;
    let env = Env::new(server.url())?;
    let state = env.dir.path().join("state");
    std::fs::create_dir_all(&state)?;
    std::fs::set_permissions(&state, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    std::fs::write(
        env.cache_path(),
        format!(
            r#"{{"checked_at":{now},"latest":"v99.0.0","html_url":null,"etag":null,"ok":true}}"#
        ),
    )?;
    std::fs::set_permissions(
        env.cache_path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o664),
    )?;
    let mut d = env.tui()?;
    wait(&d, "Projects")?;
    std::thread::sleep(Duration::from_millis(500));
    let screen = d.screen_text().to_lowercase();
    assert!(!screen.contains("99.0.0"), "{screen}");
    assert!(server.connections() >= 1, "cache must not be trusted");
    d.send(b"q")?;
    d.wait_exit(Duration::from_secs(5))?;
    Ok(())
}

#[test]
fn broken_config_disables_automatic_checks() -> Result<()> {
    let server = ReleaseServer::start(Reply::ok(release("v99.0.0")))?;
    let env = Env::new(server.url())?;
    let cfg = env.dir.path().join("config.toml");
    let text = std::fs::read_to_string(&cfg)?;
    std::fs::write(
        &cfg,
        format!("update_check = false\nupdate_chek = true\n{text}"),
    )?;
    let mut d = env.tui()?;
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(server.connections(), 0);
    assert!(!env.cache_path().exists());
    let _ = d.send(b"q");
    let _ = d.wait_exit(Duration::from_secs(5));
    Ok(())
}
