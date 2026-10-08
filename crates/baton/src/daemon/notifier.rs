//! Desktop notifications: a sink trait, the D-Bus and log backends, and a
//! worker thread so a slow notification daemon never stalls a session task.

use baton_proto::{SessionId, Status};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Notifications queued for the worker before new ones are dropped.
const QUEUE: usize = 64;

/// Environment variable choosing the sink: `log`, `off`, or anything else
/// (including unset) for D-Bus.
pub const SINK_ENV: &str = "BATON_NOTIFY_SINK";

/// One notification. `summary` and `body` are already sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub session: SessionId,
    pub status: Status,
    pub summary: String,
    pub body: String,
}

/// Somewhere notifications go. Failures are handled (logged) inside.
pub trait NotificationSink: Send + 'static {
    fn notify(&mut self, n: &Notification);
}

/// Shows notifications through the freedesktop D-Bus service.
pub struct DbusSink;

impl NotificationSink for DbusSink {
    fn notify(&mut self, n: &Notification) {
        let shown = notify_rust::Notification::new()
            .appname("Baton")
            .summary(&n.summary)
            .body(&n.body)
            .show();
        if let Err(e) = shown {
            tracing::warn!("desktop notification failed: {e}");
        }
    }
}

/// Appends `notify <session> <status>` lines to a file (used by tests).
pub struct LogSink {
    path: PathBuf,
}

impl LogSink {
    /// Logs to `<state_dir>/notifications.log`.
    pub fn new(state_dir: &Path) -> Self {
        Self {
            path: state_dir.join("notifications.log"),
        }
    }
}

impl NotificationSink for LogSink {
    fn notify(&mut self, n: &Notification) {
        use std::os::unix::fs::OpenOptionsExt;
        // The id embeds client-influenced text: keep the line a single line.
        let session: String = n.session.0.chars().filter(|c| !c.is_control()).collect();
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)
            .and_then(|mut f| writeln!(f, "notify {session} {:?}", n.status));
        if let Err(e) = result {
            tracing::warn!("writing notification log: {e}");
        }
    }
}

/// Discards notifications.
pub struct NullSink;

impl NotificationSink for NullSink {
    fn notify(&mut self, _: &Notification) {}
}

/// Which sink to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkKind {
    Dbus,
    Log,
    Off,
}

impl SinkKind {
    /// Parses the value of [`SINK_ENV`]; unknown or missing means D-Bus.
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("log") => Self::Log,
            Some("off") => Self::Off,
            _ => Self::Dbus,
        }
    }

    /// Builds the sink; `state_dir` is where the log sink writes.
    pub fn build(self, state_dir: &Path) -> Box<dyn NotificationSink> {
        match self {
            Self::Dbus => Box::new(DbusSink),
            Self::Log => Box::new(LogSink::new(state_dir)),
            Self::Off => Box::new(NullSink),
        }
    }
}

/// Handle that queues notifications for a worker thread.
#[derive(Clone)]
pub struct Notifier {
    tx: mpsc::SyncSender<Notification>,
}

impl Notifier {
    /// Starts the worker; it ends when every `Notifier` clone is dropped.
    pub fn spawn(mut sink: Box<dyn NotificationSink>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Notification>(QUEUE);
        std::thread::spawn(move || {
            for n in rx {
                sink.notify(&n);
            }
        });
        Self { tx }
    }

    /// Queues `n`; dropped (and logged) if the worker is backed up.
    pub fn send(&self, n: Notification) {
        if self.tx.try_send(n).is_err() {
            tracing::warn!("notification queue full or closed; dropping");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn note(session: &str, status: Status) -> Notification {
        Notification {
            session: SessionId(session.into()),
            status,
            summary: "s".into(),
            body: "b".into(),
        }
    }

    struct Recording(mpsc::Sender<Notification>);

    impl NotificationSink for Recording {
        fn notify(&mut self, n: &Notification) {
            let _ = self.0.send(n.clone());
        }
    }

    #[test]
    fn notifier_delivers_in_order_to_the_sink() {
        let (tx, rx) = mpsc::channel();
        let notifier = Notifier::spawn(Box::new(Recording(tx)));
        notifier.send(note("a", Status::Permission));
        notifier.send(note("b", Status::YourTurn));
        let wait = Duration::from_secs(5);
        assert_eq!(
            rx.recv_timeout(wait).ok(),
            Some(note("a", Status::Permission))
        );
        assert_eq!(
            rx.recv_timeout(wait).ok(),
            Some(note("b", Status::YourTurn))
        );
    }

    #[test]
    fn log_sink_appends_one_line_per_notification() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut sink = LogSink::new(dir.path());
        sink.notify(&note("x//tmp", Status::Permission));
        sink.notify(&note("x/\u{1b}[2J\n/evil", Status::YourTurn));
        let text = std::fs::read_to_string(dir.path().join("notifications.log")).expect("log");
        assert_eq!(
            text,
            "notify x//tmp Permission\nnotify x/[2J/evil YourTurn\n"
        );
    }

    #[test]
    fn sink_kind_parses_the_env_value() {
        assert_eq!(SinkKind::parse(Some("log")), SinkKind::Log);
        assert_eq!(SinkKind::parse(Some("off")), SinkKind::Off);
        assert_eq!(SinkKind::parse(Some("dbus")), SinkKind::Dbus);
        assert_eq!(SinkKind::parse(Some("")), SinkKind::Dbus);
        assert_eq!(SinkKind::parse(None), SinkKind::Dbus);
    }
}
