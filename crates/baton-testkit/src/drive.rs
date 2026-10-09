//! A PTY driver with a headless `vt100` screen that answers terminal queries.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use baton_core::term::Scanner;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use regex::Regex;

/// How to launch the driven command.
#[derive(Debug, Clone)]
pub struct DriveOptions {
    /// Initial screen height.
    pub rows: u16,
    /// Initial screen width.
    pub cols: u16,
    /// Extra environment variables for the child.
    pub env: Vec<(String, String)>,
    /// Working directory for the child.
    pub cwd: Option<PathBuf>,
}

impl Default for DriveOptions {
    fn default() -> Self {
        Self {
            rows: 24,
            cols: 80,
            env: Vec::new(),
            cwd: None,
        }
    }
}

struct State {
    parser: vt100::Parser,
    scanner: Scanner,
}

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// A command running in a PTY with a headless screen.
pub struct Drive {
    state: Arc<Mutex<State>>,
    writer: SharedWriter,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16),
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Drive {
    /// Spawn `argv` (an argument vector, never a shell string) in a new PTY.
    ///
    /// # Errors
    /// Fails if `argv` is empty or the PTY or child cannot be created.
    pub fn spawn(argv: &[String], opts: &DriveOptions) -> Result<Self> {
        let (program, args) = argv.split_first().context("empty command")?;
        let pair = native_pty_system().openpty(PtySize {
            rows: opts.rows,
            cols: opts.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        // portable-pty only searches PATH for relative programs, so anchor
        // `target/debug/x` style paths to our own cwd.
        let program = if program.contains('/') {
            std::path::absolute(program)?.into_os_string()
        } else {
            program.into()
        };
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        // Never let the user's proxy leak into the child.
        for k in crate::PROXY_VARS {
            cmd.env_remove(k);
        }
        cmd.env("TERM", "xterm-256color");
        for (k, v) in &opts.env {
            cmd.env(k, v);
        }
        if let Some(cwd) = &opts.cwd {
            cmd.cwd(cwd);
        }
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer: SharedWriter = Arc::new(Mutex::new(pair.master.take_writer()?));
        let state = Arc::new(Mutex::new(State {
            parser: vt100::Parser::new(opts.rows, opts.cols, 0),
            scanner: Scanner::new(),
        }));
        {
            let state = Arc::clone(&state);
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                // EOF or EIO (child gone) both end the reader.
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let replies = {
                        let mut st = lock(&state);
                        st.parser.process(&buf[..n]);
                        let (r, c) = st.parser.screen().cursor_position();
                        st.scanner.feed(&buf[..n], (r + 1, c + 1))
                    };
                    if !replies.is_empty() {
                        let mut w = lock(&writer);
                        if w.write_all(&replies).and_then(|()| w.flush()).is_err() {
                            break;
                        }
                    }
                }
            });
        }
        Ok(Self {
            state,
            writer,
            master: pair.master,
            child,
            size: (opts.rows, opts.cols),
        })
    }

    /// Write raw bytes to the child's terminal input.
    ///
    /// # Errors
    /// Fails if the PTY write fails.
    pub fn send(&self, bytes: &[u8]) -> Result<()> {
        let mut w = lock(&self.writer);
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    /// Resize the PTY and the headless screen.
    ///
    /// # Errors
    /// Fails if the PTY resize fails.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        lock(&self.state).parser.screen_mut().set_size(rows, cols);
        self.size = (rows, cols);
        Ok(())
    }

    /// Current screen size as `(rows, cols)`.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// Screen rows with trailing whitespace trimmed and trailing blank rows
    /// dropped, joined by `\n`.
    pub fn screen_text(&self) -> String {
        let st = lock(&self.state);
        let screen = st.parser.screen();
        let (_, cols) = screen.size();
        let mut rows: Vec<String> = screen
            .rows(0, cols)
            .map(|r| r.trim_end().to_string())
            .collect();
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        rows.join("\n")
    }

    /// Screen text used for `wait:` matching: like [`Drive::screen_text`], but
    /// the cursor row keeps spaces up to the cursor, so `"> "` can match.
    fn match_text(&self) -> String {
        let st = lock(&self.state);
        let screen = st.parser.screen();
        let (_, cols) = screen.size();
        let (cur_row, cur_col) = screen.cursor_position();
        let mut rows: Vec<String> = screen
            .rows(0, cols)
            .map(|r| r.trim_end().to_string())
            .collect();
        if let Some(row) = rows.get_mut(usize::from(cur_row)) {
            while row.chars().count() < usize::from(cur_col) {
                row.push(' ');
            }
        }
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        rows.join("\n")
    }

    /// The screen text between `--- screen ROWSxCOLS ---` markers.
    pub fn dump(&self) -> String {
        let (r, c) = self.size;
        format!(
            "--- screen {r}x{c} ---\n{}\n--- end screen ---\n",
            self.screen_text()
        )
    }

    /// Wait until the screen text matches `re`.
    ///
    /// # Errors
    /// Fails (including the final screen) if the timeout elapses first.
    pub fn wait_for(&self, re: &Regex, timeout: Duration) -> Result<()> {
        crate::wait_for(timeout, || re.is_match(&self.match_text()).then_some(()))
            .map_err(|e| anyhow!("{e}: waiting for /{re}/\n{}", self.dump()))
    }

    /// Wait for the child to exit and return its exit code.
    ///
    /// # Errors
    /// Fails if the child is still running after `timeout`.
    pub fn wait_exit(&mut self, timeout: Duration) -> Result<u32> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(status.exit_code());
            }
            if start.elapsed() >= timeout {
                bail!("child still running after {timeout:?}\n{}", self.dump());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Drive {
    fn drop(&mut self) {
        // Best effort: the child may already have exited.
        let _ = self.child.kill();
    }
}

/// Unescape `\r \n \t \e \\ \xNN` in a `send:` step.
///
/// # Errors
/// Fails on an unknown or truncated escape.
pub fn unescape(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut it = s.chars();
    while let Some(ch) = it.next() {
        if ch != '\\' {
            let mut b = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
            continue;
        }
        match it.next() {
            Some('r') => out.push(b'\r'),
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('e') => out.push(0x1b),
            Some('\\') => out.push(b'\\'),
            Some('x') => {
                let hex: String = it.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&hex, 16).with_context(|| format!("bad \\x{hex}"))?);
            }
            Some(other) => bail!("unknown escape \\{other}"),
            None => bail!("trailing backslash"),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::unescape;

    #[test]
    fn unescapes() {
        assert_eq!(
            unescape("a\\r\\n\\t\\e\\\\\\x41").unwrap(),
            b"a\r\n\t\x1b\\A"
        );
        assert!(unescape("\\q").is_err());
        assert!(unescape("\\xZZ").is_err());
    }
}
