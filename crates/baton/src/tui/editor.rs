//! Opens a repo in the configured editor.
//!
//! The `editor` template is split into words first and `{path}` is replaced
//! inside the individual words afterwards, so a repo path with spaces or shell
//! metacharacters always arrives as part of exactly one argv element. No shell
//! is involved.

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

/// Why the editor could not be started.
#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    /// The template has no words.
    #[error("editor command is empty")]
    Empty,
    /// The template is not valid shell-style words (e.g. unbalanced quotes).
    #[error("invalid editor command: {0}")]
    Parse(String),
    /// The program could not be started.
    #[error("cannot run {program}: {source}")]
    Spawn {
        /// The program name.
        program: String,
        /// Underlying error.
        source: std::io::Error,
    },
}

/// Splits `template` with shell word rules, then replaces `{path}` inside each
/// word with `path`.
///
/// # Errors
/// If the template is empty or has unbalanced quoting.
pub fn build_argv(template: &str, path: &str) -> Result<Vec<String>, EditorError> {
    let words = shell_words::split(template).map_err(|e| EditorError::Parse(e.to_string()))?;
    if words.is_empty() {
        return Err(EditorError::Empty);
    }
    Ok(words.iter().map(|w| w.replace("{path}", path)).collect())
}

/// Starts the editor for `path` and returns its pid.
///
/// The child runs in its own session with null stdio and `path` as its working
/// directory. It is not waited for here: a background thread reaps it when it
/// exits, so no zombie is left behind.
///
/// # Errors
/// If the template is invalid or the program cannot be started.
pub fn open(template: &str, path: &str) -> Result<u32, EditorError> {
    let argv = build_argv(template, path)?;
    let program = argv.first().cloned().unwrap_or_default();
    let mut cmd = Command::new(&program);
    cmd.args(&argv[1..])
        .current_dir(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: the closure runs between fork and exec and only calls the
    // async-signal-safe `setsid`; it allocates nothing and touches no locks.
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid()
                .map(drop)
                .map_err(std::io::Error::from)
        });
    }
    let mut child = cmd
        .spawn()
        .map_err(|source| EditorError::Spawn { program, source })?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    #[test]
    fn splits_the_template_before_substituting() {
        let argv = build_argv("code --reuse-window {path}", "/home/u/repo").expect("argv");
        assert_eq!(argv, ["code", "--reuse-window", "/home/u/repo"]);
        let argv = build_argv("ed --dir={path}/src '{path} x'", "/r").expect("argv");
        assert_eq!(argv, ["ed", "--dir=/r/src", "/r x"]);
        // No placeholder, no path.
        assert_eq!(build_argv("true", "/r").expect("argv"), ["true"]);
    }

    #[test]
    fn a_hostile_path_stays_one_argv_element() {
        for path in [
            "/tmp/my repo",
            "/tmp/a;touch pwned",
            "/tmp/$(touch pwned)",
            "/tmp/`touch pwned`",
            "/tmp/it's \"quoted\"",
            "/tmp/a|b&c>d",
            "/tmp/{path}",
        ] {
            let argv = build_argv("code -g {path}", path).expect("argv");
            assert_eq!(argv, ["code", "-g", path], "{path}");
        }
    }

    #[test]
    fn bad_templates_are_errors() {
        assert!(matches!(build_argv("", "/r"), Err(EditorError::Empty)));
        assert!(matches!(build_argv("   ", "/r"), Err(EditorError::Empty)));
        assert!(matches!(
            build_argv("code 'unclosed {path}", "/r"),
            Err(EditorError::Parse(_))
        ));
    }

    #[test]
    fn a_missing_program_is_a_spawn_error_naming_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let e = open(
            "definitely-not-an-editor-xyz {path}",
            dir.path().to_str().expect("utf8"),
        )
        .expect_err("must fail");
        assert!(matches!(e, EditorError::Spawn { .. }), "{e:?}");
        assert!(
            e.to_string().contains("definitely-not-an-editor-xyz"),
            "{e}"
        );
    }

    fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !f() {
            assert!(Instant::now() < end, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn spawns_detached_with_exact_argv_and_is_reaped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical");
        let out = root.join("out");
        let script = root.join("fake-editor");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf 'arg=%s\\n' \"$a\" >> '{out}'; done\n\
                 printf 'sid=%s pid=%s\\n' \"$(awk '{{print $6}}' /proc/self/stat)\" \"$$\" >> '{out}'\n",
                out = out.display()
            ),
        )
        .expect("write script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        // Spaces and shell metacharacters in the repo path.
        let repo = root.join("my repo;touch pwned $(touch pwned2)");
        std::fs::create_dir(&repo).expect("mkdir");
        let repo_str = repo.to_str().expect("utf8");

        let template = format!(
            "{} --flag={{path}} {{path}}",
            shell_words::quote(script.to_str().expect("utf8"))
        );
        let pid = open(&template, repo_str).expect("spawn");

        wait_until("editor output", || {
            std::fs::read_to_string(&out).is_ok_and(|t| t.contains("sid="))
        });
        let text = std::fs::read_to_string(&out).expect("read");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], format!("arg=--flag={repo_str}"));
        assert_eq!(lines[1], format!("arg={repo_str}"));
        assert_eq!(lines[2], format!("sid={pid} pid={pid}"), "own session");
        assert!(!root.join("pwned").exists() && !root.join("pwned2").exists());
        // Reaped: the pid disappears instead of lingering as a zombie.
        wait_until("reaping", || {
            !std::path::Path::new(&format!("/proc/{pid}")).exists()
        });
    }
}
