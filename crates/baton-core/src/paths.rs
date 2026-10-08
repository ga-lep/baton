//! Filesystem locations, honouring `BATON_CONFIG`, `BATON_STATE_DIR` and `BATON_RUNTIME_DIR`.

use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

type Getenv<'a> = &'a dyn Fn(&str) -> Option<String>;

fn real_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn non_empty(get: Getenv<'_>, key: &str) -> Option<PathBuf> {
    get(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// `$HOME`, else the home directory from the passwd entry of the current user.
///
/// Errors if neither is available (we never guess `/`).
fn home(get: Getenv<'_>) -> io::Result<PathBuf> {
    if let Some(h) = non_empty(get, "HOME") {
        return Ok(h);
    }
    match nix::unistd::User::from_uid(nix::unistd::getuid()) {
        Ok(Some(user)) => Ok(user.dir),
        Ok(None) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "HOME is not set and the current user has no passwd entry",
        )),
        Err(e) => Err(io::Error::other(format!(
            "HOME is not set and the passwd lookup failed: {e}"
        ))),
    }
}

/// Config file path (`BATON_CONFIG`, else `$XDG_CONFIG_HOME/baton/config.toml`,
/// else `~/.config/baton/config.toml`).
///
/// # Errors
/// If the home directory is needed but cannot be determined.
pub fn config_file_with(get: Getenv<'_>) -> io::Result<PathBuf> {
    if let Some(p) = non_empty(get, "BATON_CONFIG") {
        return Ok(p);
    }
    let base = match non_empty(get, "XDG_CONFIG_HOME") {
        Some(x) => x,
        None => home(get)?.join(".config"),
    };
    Ok(base.join("baton/config.toml"))
}

/// State directory (`BATON_STATE_DIR`, else `$XDG_STATE_HOME/baton`, else `~/.local/state/baton`).
///
/// # Errors
/// If the home directory is needed but cannot be determined.
pub fn state_dir_with(get: Getenv<'_>) -> io::Result<PathBuf> {
    if let Some(p) = non_empty(get, "BATON_STATE_DIR") {
        return Ok(p);
    }
    let base = match non_empty(get, "XDG_STATE_HOME") {
        Some(x) => x,
        None => home(get)?.join(".local/state"),
    };
    Ok(base.join("baton"))
}

/// Runtime directory (`BATON_RUNTIME_DIR`, else `$XDG_RUNTIME_DIR/baton`, else `/tmp/baton-<uid>`).
pub fn runtime_dir_with(get: Getenv<'_>, uid: u32) -> PathBuf {
    non_empty(get, "BATON_RUNTIME_DIR").unwrap_or_else(|| match non_empty(get, "XDG_RUNTIME_DIR") {
        Some(x) => x.join("baton"),
        None => PathBuf::from(format!("/tmp/baton-{uid}")),
    })
}

/// Config file path from the process environment.
///
/// # Errors
/// If the home directory cannot be determined.
pub fn config_file() -> io::Result<PathBuf> {
    config_file_with(&real_env)
}

/// State directory from the process environment.
///
/// # Errors
/// If the home directory cannot be determined.
pub fn state_dir() -> io::Result<PathBuf> {
    state_dir_with(&real_env)
}

/// Runtime directory from the process environment.
pub fn runtime_dir() -> PathBuf {
    runtime_dir_with(&real_env, nix::unistd::getuid().as_raw())
}

/// Creates the runtime directory (mode 0700) if missing and returns it.
///
/// Whether freshly created or pre-existing, the directory must be a real
/// directory (not a symlink), owned by the current user, with no group/other
/// permission bits; otherwise this fails with `PermissionDenied`. This guards
/// against another local user pre-creating the `/tmp/baton-<uid>` fallback.
pub fn ensure_runtime_dir() -> io::Result<PathBuf> {
    let dir = runtime_dir();
    ensure_private_dir(&dir, nix::unistd::getuid().as_raw())?;
    Ok(dir)
}

fn ensure_private_dir(dir: &Path, uid: u32) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    let deny = |why: String| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to use runtime dir {}: {why}", dir.display()),
        ))
    };
    if !meta.file_type().is_dir() {
        return deny("not a real directory (symlink or other file type)".into());
    }
    if meta.uid() != uid {
        return deny(format!("owned by uid {}, expected {uid}", meta.uid()));
    }
    if meta.mode() & 0o077 != 0 {
        return deny(format!(
            "mode {:o} grants group/other access (need 0700)",
            meta.mode() & 0o777
        ));
    }
    Ok(())
}

/// Daemon socket: `<runtime_dir>/baton.sock`.
pub fn socket_path() -> PathBuf {
    runtime_dir().join("baton.sock")
}

/// Injected Claude Code settings: `<runtime_dir>/hooks.json`.
pub fn hooks_json_path() -> PathBuf {
    runtime_dir().join("hooks.json")
}

/// Persisted session metadata: `<state_dir>/state.json`.
///
/// # Errors
/// If the home directory cannot be determined.
pub fn state_json_path() -> io::Result<PathBuf> {
    Ok(state_dir()?.join("state.json"))
}

/// Daemon log: `<state_dir>/daemon.log`.
///
/// # Errors
/// If the home directory cannot be determined.
pub fn log_path() -> io::Result<PathBuf> {
    Ok(state_dir()?.join("daemon.log"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn overrides_win() {
        let e = env(&[
            ("BATON_CONFIG", "/c.toml"),
            ("BATON_STATE_DIR", "/s"),
            ("BATON_RUNTIME_DIR", "/r"),
            ("HOME", "/h"),
            ("XDG_RUNTIME_DIR", "/x"),
        ]);
        assert_eq!(config_file_with(&e).unwrap(), PathBuf::from("/c.toml"));
        assert_eq!(state_dir_with(&e).unwrap(), PathBuf::from("/s"));
        assert_eq!(runtime_dir_with(&e, 1), PathBuf::from("/r"));
    }

    #[test]
    fn xdg_defaults() {
        let e = env(&[("HOME", "/h"), ("XDG_RUNTIME_DIR", "/run/user/1")]);
        assert_eq!(
            config_file_with(&e).unwrap(),
            PathBuf::from("/h/.config/baton/config.toml")
        );
        assert_eq!(
            state_dir_with(&e).unwrap(),
            PathBuf::from("/h/.local/state/baton")
        );
        assert_eq!(runtime_dir_with(&e, 1), PathBuf::from("/run/user/1/baton"));
        let e = env(&[
            ("HOME", "/h"),
            ("XDG_CONFIG_HOME", "/xc"),
            ("XDG_STATE_HOME", "/xs"),
        ]);
        assert_eq!(
            config_file_with(&e).unwrap(),
            PathBuf::from("/xc/baton/config.toml")
        );
        assert_eq!(state_dir_with(&e).unwrap(), PathBuf::from("/xs/baton"));
    }

    fn mkdir(path: &Path, mode: u32) {
        std::fs::DirBuilder::new().mode(mode).create(path).unwrap();
    }

    fn uid() -> u32 {
        nix::unistd::getuid().as_raw()
    }

    #[test]
    fn private_dir_fresh_is_created_0700() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("a/b");
        ensure_private_dir(&d, uid()).unwrap();
        let mode = std::fs::metadata(&d).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn private_dir_existing_0700_ok() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("a");
        mkdir(&d, 0o700);
        ensure_private_dir(&d, uid()).unwrap();
    }

    #[test]
    fn private_dir_rejects_loose_mode() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("a");
        mkdir(&d, 0o755);
        // DirBuilder is subject to umask; force the mode.
        std::fs::set_permissions(&d, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let e = ensure_private_dir(&d, uid()).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn private_dir_rejects_symlink() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real");
        mkdir(&real, 0o700);
        let link = t.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let e = ensure_private_dir(&link, uid()).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn private_dir_rejects_other_owner() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("a");
        mkdir(&d, 0o700);
        let e = ensure_private_dir(&d, uid().wrapping_add(1)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn home_falls_back_to_passwd_not_root() {
        let user = nix::unistd::User::from_uid(nix::unistd::getuid())
            .unwrap()
            .expect("current user has a passwd entry");
        let got = config_file_with(&env(&[])).unwrap();
        assert_eq!(got, user.dir.join(".config/baton/config.toml"));
        let got = state_dir_with(&env(&[("HOME", "")])).unwrap();
        assert_eq!(got, user.dir.join(".local/state/baton"));
    }

    #[test]
    fn runtime_falls_back_to_tmp_uid() {
        assert_eq!(
            runtime_dir_with(&env(&[]), 1000),
            PathBuf::from("/tmp/baton-1000")
        );
    }
}
