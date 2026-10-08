//! Filesystem locations, honouring `BATON_CONFIG`, `BATON_STATE_DIR` and `BATON_RUNTIME_DIR`.

use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

type Getenv<'a> = &'a dyn Fn(&str) -> Option<String>;

fn real_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn non_empty(get: Getenv<'_>, key: &str) -> Option<PathBuf> {
    get(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn home(get: Getenv<'_>) -> PathBuf {
    non_empty(get, "HOME").unwrap_or_else(|| PathBuf::from("/"))
}

/// Config file path (`BATON_CONFIG`, else `$XDG_CONFIG_HOME/baton/config.toml`).
pub fn config_file_with(get: Getenv<'_>) -> PathBuf {
    non_empty(get, "BATON_CONFIG").unwrap_or_else(|| {
        non_empty(get, "XDG_CONFIG_HOME")
            .unwrap_or_else(|| home(get).join(".config"))
            .join("baton/config.toml")
    })
}

/// State directory (`BATON_STATE_DIR`, else `$XDG_STATE_HOME/baton`, else `~/.local/state/baton`).
pub fn state_dir_with(get: Getenv<'_>) -> PathBuf {
    non_empty(get, "BATON_STATE_DIR").unwrap_or_else(|| {
        non_empty(get, "XDG_STATE_HOME")
            .unwrap_or_else(|| home(get).join(".local/state"))
            .join("baton")
    })
}

/// Runtime directory (`BATON_RUNTIME_DIR`, else `$XDG_RUNTIME_DIR/baton`, else `/tmp/baton-<uid>`).
pub fn runtime_dir_with(get: Getenv<'_>, uid: u32) -> PathBuf {
    non_empty(get, "BATON_RUNTIME_DIR").unwrap_or_else(|| match non_empty(get, "XDG_RUNTIME_DIR") {
        Some(x) => x.join("baton"),
        None => PathBuf::from(format!("/tmp/baton-{uid}")),
    })
}

/// Config file path from the process environment.
pub fn config_file() -> PathBuf {
    config_file_with(&real_env)
}

/// State directory from the process environment.
pub fn state_dir() -> PathBuf {
    state_dir_with(&real_env)
}

/// Runtime directory from the process environment.
pub fn runtime_dir() -> PathBuf {
    runtime_dir_with(&real_env, nix::unistd::getuid().as_raw())
}

/// Creates the runtime directory (mode 0700) if missing and returns it.
pub fn ensure_runtime_dir() -> io::Result<PathBuf> {
    let dir = runtime_dir();
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    Ok(dir)
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
pub fn state_json_path() -> PathBuf {
    state_dir().join("state.json")
}

/// Daemon log: `<state_dir>/daemon.log`.
pub fn log_path() -> PathBuf {
    state_dir().join("daemon.log")
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
        assert_eq!(config_file_with(&e), PathBuf::from("/c.toml"));
        assert_eq!(state_dir_with(&e), PathBuf::from("/s"));
        assert_eq!(runtime_dir_with(&e, 1), PathBuf::from("/r"));
    }

    #[test]
    fn xdg_defaults() {
        let e = env(&[("HOME", "/h"), ("XDG_RUNTIME_DIR", "/run/user/1")]);
        assert_eq!(
            config_file_with(&e),
            PathBuf::from("/h/.config/baton/config.toml")
        );
        assert_eq!(state_dir_with(&e), PathBuf::from("/h/.local/state/baton"));
        assert_eq!(runtime_dir_with(&e, 1), PathBuf::from("/run/user/1/baton"));
        let e = env(&[
            ("HOME", "/h"),
            ("XDG_CONFIG_HOME", "/xc"),
            ("XDG_STATE_HOME", "/xs"),
        ]);
        assert_eq!(config_file_with(&e), PathBuf::from("/xc/baton/config.toml"));
        assert_eq!(state_dir_with(&e), PathBuf::from("/xs/baton"));
    }

    #[test]
    fn runtime_falls_back_to_tmp_uid() {
        assert_eq!(
            runtime_dir_with(&env(&[]), 1000),
            PathBuf::from("/tmp/baton-1000")
        );
    }
}
