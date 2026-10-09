//! A tiny threaded HTTP/1.1 server that impersonates the GitHub releases API.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Canned reply sent to every request.
#[derive(Debug, Clone)]
pub struct Reply {
    /// HTTP status code.
    pub status: u16,
    /// Response body.
    pub body: String,
    /// `ETag` header, when set.
    pub etag: Option<String>,
    /// `Location` header, when set (for redirects).
    pub location: Option<String>,
    /// Pause before answering (a very long delay emulates a hung server).
    pub delay: Duration,
}

impl Reply {
    /// A `200` reply carrying `body`.
    pub fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            etag: None,
            location: None,
            delay: Duration::ZERO,
        }
    }

    /// An empty reply with `status`.
    pub fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::ok("")
        }
    }

    /// A `302` redirect to `location`.
    pub fn redirect(location: impl Into<String>) -> Self {
        Self {
            location: Some(location.into()),
            ..Self::status(302)
        }
    }

    /// A reply that accepts the connection but answers only after a long time.
    pub fn hang() -> Self {
        Self {
            delay: Duration::from_secs(30),
            ..Self::ok("")
        }
    }
}

#[derive(Default)]
struct Shared {
    connections: usize,
    requests: Vec<Vec<(String, String)>>,
}

/// A running fake release server on `127.0.0.1:0`.
pub struct ReleaseServer {
    addr: SocketAddr,
    shared: Arc<Mutex<Shared>>,
}

impl ReleaseServer {
    /// Starts the server; it lives (detached) until the process exits.
    ///
    /// # Errors
    /// If binding the listener fails.
    pub fn start(reply: Reply) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Mutex::new(Shared::default()));
        let state = Arc::clone(&shared);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Ok(mut s) = state.lock() {
                    s.connections += 1;
                }
                let state = Arc::clone(&state);
                let reply = reply.clone();
                std::thread::spawn(move || {
                    let _ = serve(stream, &reply, &state);
                });
            }
        });
        Ok(Self { addr, shared })
    }

    /// URL of the server's release endpoint.
    pub fn url(&self) -> String {
        format!("http://{}/repos/o/r/releases/latest", self.addr)
    }

    /// Number of TCP connections accepted so far.
    pub fn connections(&self) -> usize {
        self.shared.lock().map_or(0, |s| s.connections)
    }

    /// Headers (lower-cased names) of every request received so far.
    pub fn requests(&self) -> Vec<Vec<(String, String)>> {
        self.shared
            .lock()
            .map(|s| s.requests.clone())
            .unwrap_or_default()
    }
}

fn serve(stream: TcpStream, reply: &Reply, state: &Mutex<Shared>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?; // request line
    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.trim_end().split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
        }
    }
    if let Ok(mut s) = state.lock() {
        s.requests.push(headers);
    }
    std::thread::sleep(reply.delay);
    let mut out = stream;
    let etag = reply
        .etag
        .as_ref()
        .map_or(String::new(), |e| format!("ETag: {e}\r\n"));
    let location = reply
        .location
        .as_ref()
        .map_or(String::new(), |l| format!("Location: {l}\r\n"));
    write!(
        out,
        "HTTP/1.1 {} X\r\nContent-Type: application/json\r\n{etag}{location}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        reply.status,
        reply.body.len(),
        reply.body
    )?;
    out.flush()
}
