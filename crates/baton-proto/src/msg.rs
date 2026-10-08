//! Message and data types exchanged over the Baton IPC socket.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Wire protocol version. Bump on any incompatible change to these types.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum size of one frame (16 MiB).
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Stable baton session id: `"<project>/<repo-path>"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    /// Builds the id from a project name and a repo path, canonicalizing the
    /// path (the caller must already have expanded `~` and env vars).
    ///
    /// # Errors
    /// Returns the I/O error if the path cannot be canonicalized.
    pub fn from_repo(project: &str, repo: &Path) -> io::Result<Self> {
        let canon = repo.canonicalize()?;
        Ok(Self(format!("{project}/{}", canon.display())))
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Kind of peer that opened the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Tui,
    Hook,
    Ctl,
}

/// Frames sent from a client (TUI, hook or ctl) to the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello {
        version: u32,
        role: Role,
    },
    Attach {
        rows: u16,
        cols: u16,
    },
    OpenProject {
        name: String,
    },
    Restart {
        session: SessionId,
    },
    Input {
        session: SessionId,
        bytes: Vec<u8>,
    },
    /// Applies to all sessions.
    Resize {
        rows: u16,
        cols: u16,
    },
    MarkViewed {
        session: SessionId,
    },
    ClientView {
        on_screen: Option<SessionId>,
        terminal_focused: bool,
    },
    GetScrollback {
        session: SessionId,
        start: u32,
        count: u32,
    },
    Detach,
    Hook {
        baton_session: SessionId,
        event: String,
        payload_json: String,
    },
    Status,
    Shutdown,
}

/// Frames sent from the daemon to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DaemonMsg {
    Welcome {
        version: u32,
        pid: u32,
    },
    VersionMismatch {
        daemon_version: u32,
    },
    SessionList(Vec<SessionInfo>),
    Snapshot {
        session: SessionId,
        rows: u16,
        cols: u16,
        bytes: Vec<u8>,
    },
    Output {
        session: SessionId,
        bytes: Vec<u8>,
    },
    StatusChanged {
        session: SessionId,
        status: Status,
    },
    UsageUpdated {
        session: SessionId,
        usage: Usage,
    },
    /// Formatted scrollback rows starting at line `start`.
    Scrollback {
        session: SessionId,
        start: u32,
        rows: Vec<Vec<u8>>,
    },
    DaemonStatus {
        pid: u32,
        version: u32,
        sessions: Vec<SessionInfo>,
    },
    Error {
        message: String,
    },
}

/// Per-session summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: SessionId,
    pub project: String,
    pub repo: String,
    pub profile: Option<String>,
    pub status: Status,
    pub claude_session_id: Option<String>,
    pub model: Option<String>,
    /// Unix timestamp, seconds.
    pub started_at: u64,
    pub exit_code: Option<i32>,
    pub usage: Option<Usage>,
}

/// Session status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Starting,
    Running,
    Permission,
    YourTurn,
    Idle,
    Exited(i32),
    Unknown,
}

/// Token usage and estimated cost.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub context_pct: Option<f32>,
    pub cost_usd: Option<f64>,
    pub model: Option<String>,
}
