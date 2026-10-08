//! Launching a session's child process inside a fresh PTY.

use anyhow::{Context, Result, bail};
use baton_core::config::SessionSpec;
use baton_proto::SessionId;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::Path;

/// A running child and the master side of its PTY.
pub struct Spawned {
    pub master: Box<dyn MasterPty + Send>,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub pid: u32,
}

/// Builds the child command from an argument vector (never a shell string).
///
/// The command is the profile argv followed by the repo `args`, runs with
/// `cwd` set to the repo (portable-pty would otherwise use `$HOME`), and gets
/// the inherited environment, the profile environment, then `BATON_SESSION`
/// and `BATON_SOCK`, which the profile cannot override.
///
/// # Errors
/// If the profile argv is empty.
pub fn build_command(spec: &SessionSpec, id: &SessionId, sock: &Path) -> Result<CommandBuilder> {
    let (program, rest) = spec.argv.split_first().context("empty profile command")?;
    // A relative path containing '/' would otherwise resolve against the new
    // cwd; anchor it to the daemon's own directory like a shell would.
    let program = if program.contains('/') {
        std::path::absolute(program)?.into_os_string()
    } else {
        program.into()
    };
    let mut cmd = CommandBuilder::new(program);
    cmd.args(rest);
    cmd.args(&spec.args);
    cmd.cwd(&spec.repo);
    cmd.env("TERM", "xterm-256color");
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    cmd.env("BATON_SESSION", &id.0);
    cmd.env("BATON_SOCK", sock);
    Ok(cmd)
}

/// Opens a PTY of `size` (`rows`, `cols`) and starts the session's child in
/// its own session and process group.
///
/// # Errors
/// If the PTY cannot be opened or the program cannot be started.
pub fn spawn(spec: &SessionSpec, id: &SessionId, sock: &Path, size: (u16, u16)) -> Result<Spawned> {
    let cmd = build_command(spec, id, sock)?;
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: size.0,
            cols: size.1,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("opening pty")?;
    // portable-pty runs setsid() in the child, making it a session and
    // process-group leader whose controlling terminal is the PTY.
    let child = pair
        .slave
        .spawn_command(cmd)
        .with_context(|| format!("starting {}", spec.argv.join(" ")))?;
    drop(pair.slave);
    let Some(pid) = child.process_id() else {
        bail!("child has no pid");
    };
    Ok(Spawned {
        reader: pair.master.try_clone_reader().context("pty reader")?,
        writer: pair.master.take_writer().context("pty writer")?,
        master: pair.master,
        child,
        pid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    fn spec() -> SessionSpec {
        SessionSpec {
            project: "x".into(),
            repo: PathBuf::from("/tmp"),
            profile: "p".into(),
            argv: vec!["bash".into(), "--norc".into()],
            args: vec!["-i".into()],
            env: BTreeMap::from([
                ("FOO".to_owned(), "bar".to_owned()),
                ("BATON_SESSION".to_owned(), "spoofed".to_owned()),
            ]),
        }
    }

    #[test]
    fn command_is_argv_with_repo_cwd_and_baton_env() {
        let id = SessionId("x//tmp".into());
        let cmd = build_command(&spec(), &id, Path::new("/run/baton.sock")).unwrap();
        let argv: Vec<_> = cmd.get_argv().iter().map(|a| a.to_string_lossy()).collect();
        assert_eq!(argv, ["bash", "--norc", "-i"]);
        assert_eq!(
            cmd.get_cwd().map(|c| c.as_os_str()),
            Some(OsStr::new("/tmp"))
        );
        assert_eq!(cmd.get_env("FOO"), Some(OsStr::new("bar")));
        assert_eq!(cmd.get_env("BATON_SESSION"), Some(OsStr::new("x//tmp")));
        assert_eq!(
            cmd.get_env("BATON_SOCK"),
            Some(OsStr::new("/run/baton.sock"))
        );
        assert_eq!(cmd.get_env("TERM"), Some(OsStr::new("xterm-256color")));
    }

    #[test]
    fn empty_argv_is_an_error() {
        let mut s = spec();
        s.argv.clear();
        let id = SessionId("x//tmp".into());
        assert!(build_command(&s, &id, Path::new("/s")).is_err());
    }
}
