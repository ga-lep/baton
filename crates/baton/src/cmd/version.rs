//! `baton version [--check]`.

use crate::update::{self, Checked};
use baton_core::config::Config;
use baton_core::paths;
use baton_core::update::Outcome;
use baton_proto::PROTOCOL_VERSION;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One-line message for a completed check.
pub fn render(checked: &Checked) -> String {
    match &checked.outcome {
        Outcome::UpToDate => format!("baton {VERSION} is up to date"),
        Outcome::Newer { latest } => {
            let url = checked.html_url.as_deref().unwrap_or("");
            format!("baton {latest} is available (you have {VERSION}): {url}")
        }
        Outcome::Unknown(why) => format!("could not check for updates: {why}"),
    }
}

fn config_flag() -> bool {
    let getenv = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    paths::config_file()
        .ok()
        .and_then(|p| Config::load(&p, &getenv).ok())
        .is_none_or(|c| c.update_check)
}

/// Runs `baton version`; with `check`, queries GitHub and exits 1 on failure.
pub fn run(check: bool) -> ExitCode {
    if !check {
        println!("baton {VERSION} (protocol {PROTOCOL_VERSION})");
        return ExitCode::SUCCESS;
    }
    let result = update::check(true, update::CLI_TIMEOUT);
    let disabled = !baton_core::update::enabled(config_flag(), &|k| std::env::var(k).ok());
    let note = if disabled {
        " (automatic checks are disabled)"
    } else {
        ""
    };
    match result {
        Ok(checked) if matches!(checked.outcome, Outcome::Unknown(_)) => {
            println!("{}{note}", render(&checked));
            ExitCode::FAILURE
        }
        Ok(checked) => {
            println!("{}{note}", render(&checked));
            ExitCode::SUCCESS
        }
        Err(reason) => {
            println!("could not check for updates: {reason}{note}");
            ExitCode::FAILURE
        }
    }
}
