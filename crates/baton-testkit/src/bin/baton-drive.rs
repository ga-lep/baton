//! `baton-drive`: run a command in a PTY, script steps, print screen dumps.

use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use baton_testkit::{Drive, DriveOptions, unescape};
use clap::Parser;
use regex::Regex;

#[derive(Parser)]
#[command(about = "Run a command in a PTY and drive it with scripted steps")]
struct Args {
    /// Initial size as ROWSxCOLS.
    #[arg(long, default_value = "24x80")]
    size: String,
    /// Per-step timeout in milliseconds.
    #[arg(long, default_value_t = 10_000)]
    timeout_ms: u64,
    /// Step: send:<text>, wait:<regex>, resize:ROWSxCOLS, sleep:<ms>, dump,
    /// expect-exit:<code>.
    #[arg(long = "step")]
    steps: Vec<String>,
    /// Command and arguments.
    #[arg(last = true, required = true)]
    cmd: Vec<String>,
}

fn parse_size(s: &str) -> Result<(u16, u16)> {
    let (r, c) = s.split_once('x').context("size must be ROWSxCOLS")?;
    Ok((r.parse()?, c.parse()?))
}

fn run(args: &Args) -> Result<()> {
    let (rows, cols) = parse_size(&args.size)?;
    let timeout = Duration::from_millis(args.timeout_ms);
    let mut drive = Drive::spawn(
        &args.cmd,
        &DriveOptions {
            rows,
            cols,
            ..DriveOptions::default()
        },
    )?;
    for step in &args.steps {
        let (kind, arg) = step.split_once(':').unwrap_or((step.as_str(), ""));
        let res: Result<()> = (|| {
            match kind {
                "send" => drive.send(&unescape(arg)?)?,
                "wait" => drive.wait_for(&Regex::new(arg)?, timeout)?,
                "resize" => {
                    let (r, c) = parse_size(arg)?;
                    drive.resize(r, c)?;
                }
                "sleep" => std::thread::sleep(Duration::from_millis(arg.parse()?)),
                "dump" => print!("{}", drive.dump()),
                "expect-exit" => {
                    let want: u32 = arg.parse()?;
                    let got = drive.wait_exit(timeout)?;
                    if got != want {
                        bail!("expected exit {want}, got {got}\n{}", drive.dump());
                    }
                }
                other => bail!("unknown step kind {other:?}"),
            }
            Ok(())
        })();
        if let Err(e) = res {
            return Err(e.context(format!("step {step:?}")));
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("baton-drive: {e:#}");
            ExitCode::from(1)
        }
    }
}
