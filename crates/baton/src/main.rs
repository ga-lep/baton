//! `baton` binary entry point.

mod cli;
mod client;
mod cmd;
mod daemon;
mod logging;
mod spike;
#[allow(dead_code)] // consumed by the TUI client in later tasks
mod term;
mod tui;

use clap::Parser;
use cli::{Cli, Command, ConfigAction};
use std::process::ExitCode;

/// Exit code for subcommands that are not implemented yet.
const EXIT_NOT_IMPLEMENTED: u8 = 2;

fn main() -> ExitCode {
    run(Cli::parse())
}

fn run(cli: Cli) -> ExitCode {
    match cli.command {
        // The hook must never print or fail: its stdout would reach Claude's context.
        Some(Command::Hook { .. }) => ExitCode::SUCCESS,
        Some(Command::Spike { cmd }) => match spike::run(&cmd) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("baton spike: {e:#}");
                ExitCode::FAILURE
            }
        },
        Some(Command::Config {
            action: ConfigAction::Check,
        }) => cmd::config::check(),
        Some(Command::Daemon { action }) => cmd::daemon::run(&action),
        Some(Command::Debug { args }) => cmd::debug::run(&args),
        None => match tui::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("baton: {e:#}");
                ExitCode::FAILURE
            }
        },
        _ => not_implemented(),
    }
}

fn not_implemented() -> ExitCode {
    eprintln!("not implemented yet");
    ExitCode::from(EXIT_NOT_IMPLEMENTED)
}
