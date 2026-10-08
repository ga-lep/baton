//! `baton` binary entry point.

mod cli;

use clap::Parser;
use cli::{Cli, Command};
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
        _ => not_implemented(),
    }
}

fn not_implemented() -> ExitCode {
    eprintln!("not implemented yet");
    ExitCode::from(EXIT_NOT_IMPLEMENTED)
}
