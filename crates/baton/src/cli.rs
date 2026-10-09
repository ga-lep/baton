//! Command-line interface definition for `baton`.

use clap::{Parser, Subcommand};

/// Baton: a TUI that supervises interactive Claude Code sessions.
#[derive(Debug, Parser)]
#[command(name = "baton", version, about)]
pub struct Cli {
    /// Subcommand to run; without one, the TUI starts.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage the background daemon.
    Daemon {
        /// Daemon action.
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Claude Code hook entry point; reads the event JSON on stdin.
    Hook {
        /// Hook event name, e.g. `Stop`.
        #[arg(value_name = "EVENT")]
        event: String,
    },
    /// Claude Code status line entry point; relays the quota to the daemon and
    /// runs the `statusline` command from the config.
    Statusline,
    /// Run the embedding spike against a child command (defaults to `claude`).
    Spike {
        /// Command and arguments to embed.
        #[arg(last = true)]
        cmd: Vec<String>,
    },
    /// Check the local environment and that each profile fires Baton's hooks.
    Doctor {
        /// Skip the hook probe (which launches each profile's command briefly).
        #[arg(long)]
        no_probe: bool,
    },
    /// Print the version; with `--check`, look for a newer release.
    Version {
        /// Query GitHub for the latest release (ignores the cache).
        #[arg(long)]
        check: bool,
    },
    /// Inspect the configuration.
    Config {
        /// Config action.
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Debug client for the daemon (hidden).
    #[command(hide = true)]
    Debug {
        /// Debug subcommand and its arguments.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

/// Daemon actions.
#[derive(Debug, Subcommand)]
pub enum DaemonAction {
    /// Start the daemon.
    Start {
        /// Run attached instead of detaching.
        #[arg(long, hide = true)]
        foreground: bool,
    },
    /// Stop the daemon.
    Stop,
    /// Show daemon status.
    Status,
}

/// Config actions.
#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Validate the config and print resolved sessions.
    Check,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn clap_command_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn help_lists_public_subcommands_but_not_debug() {
        let help = Cli::command().render_help().to_string();
        for name in ["daemon", "hook", "spike", "doctor", "config", "version"] {
            assert!(help.contains(name), "missing {name}");
        }
        assert!(!help.contains("debug"));
    }

    #[test]
    fn hidden_debug_parses() {
        let cli = Cli::try_parse_from(["baton", "debug", "open", "x"]).expect("parses");
        assert!(matches!(cli.command, Some(Command::Debug { .. })));
    }

    #[test]
    fn hook_takes_positional_event() {
        let cli = Cli::try_parse_from(["baton", "hook", "Stop"]).expect("parses");
        assert!(matches!(cli.command, Some(Command::Hook { event }) if event == "Stop"));
    }

    #[test]
    fn daemon_has_start_stop_status() {
        for sub in ["start", "stop", "status"] {
            Cli::try_parse_from(["baton", "daemon", sub]).expect("parses");
        }
    }
}
