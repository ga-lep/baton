//! `baton config check`: validate the config and print the resolved sessions.

use baton_core::config::{Config, ConfigError};
use baton_core::paths;
use std::process::ExitCode;

/// Renders one line per session: `<project>  <repo>  profile=<p>  cmd=<argv>  env=<keys>`.
pub fn render(config: &Config) -> Vec<String> {
    config
        .projects
        .iter()
        .flat_map(|p| &p.sessions)
        .map(|s| {
            let keys: Vec<&str> = s.env.keys().map(String::as_str).collect();
            format!(
                "{}  {}  profile={}  cmd={}  env={}",
                s.project,
                s.repo.display(),
                s.profile,
                serde_json::to_string(&s.argv).unwrap_or_default(),
                keys.join(",")
            )
        })
        .collect()
}

fn load() -> Result<Config, ConfigError> {
    Config::load(&paths::config_file(), &|k| {
        std::env::var(k).ok().filter(|v| !v.is_empty())
    })
}

/// Runs `baton config check`; exits 1 on an invalid config.
pub fn check() -> ExitCode {
    match load() {
        Ok(config) => {
            for line in render(&config) {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_empty_env_and_argv() {
        let c = Config::parse("[[projects]]\nname=\"a\"\nrepos=[{path=\"/r\"}]\n", &|_| {
            None
        })
        .expect("ok");
        assert_eq!(
            render(&c),
            ["a  /r  profile=default  cmd=[\"claude\"]  env="]
        );
    }
}
