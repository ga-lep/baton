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
mod update;

use clap::Parser;
use cli::{Cli, Command, ConfigAction};
use std::process::ExitCode;

fn main() -> ExitCode {
    // `baton hook` must stay silent and exit 0 whatever it is given, which
    // clap (exit code 2 on bad arguments) cannot promise: dispatch it first.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "hook") {
        return cmd::hook::run(&args[2..]);
    }
    if args.get(1).is_some_and(|a| a == "statusline") {
        return cmd::statusline::run();
    }
    run(Cli::parse())
}

fn run(cli: Cli) -> ExitCode {
    match cli.command {
        // The hook must never print or fail: its stdout would reach Claude's context.
        Some(Command::Hook { .. }) => ExitCode::SUCCESS,
        Some(Command::Statusline) => cmd::statusline::run(),
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
        Some(Command::Version { check }) => cmd::version::run(check),
        Some(Command::Doctor { no_probe }) => cmd::doctor::run(no_probe),
        None => match tui::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("baton: {e:#}");
                ExitCode::FAILURE
            }
        },
    }
}
