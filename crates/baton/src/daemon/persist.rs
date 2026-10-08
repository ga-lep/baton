//! The daemon's copy of `state.json`: updated by sessions, written back
//! atomically with a debounce.

use baton_core::state::{self, PersistedSession, State};
use baton_proto::SessionId;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

/// Changes made within this long of each other are written together.
pub const DEBOUNCE: Duration = Duration::from_millis(250);

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Shared persisted state. Without a path it only remembers in memory.
pub struct Store {
    path: Option<PathBuf>,
    state: Mutex<State>,
    /// Serializes writes so two flushes never share a temp file.
    write: Mutex<()>,
    dirty: Notify,
}

impl Store {
    /// A store that is never written to disk.
    pub fn in_memory() -> Arc<Self> {
        Arc::new(Self {
            path: None,
            state: Mutex::new(State::default()),
            write: Mutex::new(()),
            dirty: Notify::new(),
        })
    }

    /// Loads `path` tolerantly (a corrupt file is set aside and logged) and
    /// keeps writing back to it.
    pub fn load(path: PathBuf) -> Arc<Self> {
        let loaded = state::load(&path, now_secs());
        if let Some(problem) = &loaded.problem {
            tracing::warn!("ignoring unusable state file, starting empty: {problem}");
        } else {
            tracing::info!(sessions = loaded.state.sessions.len(), "state loaded");
        }
        Arc::new(Self {
            path: Some(path),
            state: Mutex::new(loaded.state),
            write: Mutex::new(()),
            dirty: Notify::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What is remembered about `id`.
    pub fn get(&self, id: &SessionId) -> Option<PersistedSession> {
        self.lock().sessions.get(&id.0).cloned()
    }

    /// Remembers `entry` for `id` and schedules a write.
    pub fn put(&self, id: &SessionId, entry: PersistedSession) {
        self.lock().sessions.insert(id.0.clone(), entry);
        self.dirty.notify_one();
    }

    /// Writes the state now (a no-op for an in-memory store).
    pub fn flush(&self) {
        let Some(path) = &self.path else { return };
        let _serial = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot = self.lock().clone();
        if let Err(e) = state::save(path, &snapshot) {
            tracing::warn!("cannot write {}: {e}", path.display());
        }
    }

    /// Writes changes at most once per [`DEBOUNCE`]; never returns.
    pub async fn run_writer(self: Arc<Self>) {
        loop {
            self.dirty.notified().await;
            tokio::time::sleep(DEBOUNCE).await;
            let this = Arc::clone(&self);
            if tokio::task::spawn_blocking(move || this.flush())
                .await
                .is_err()
            {
                tracing::warn!("state writer task panicked");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baton_proto::Status;

    fn entry(id: &str) -> PersistedSession {
        PersistedSession {
            project: "x".into(),
            repo: "/r".into(),
            profile: None,
            claude_session_id: Some(id.into()),
            transcript_path: None,
            last_status: Status::Idle,
            updated_at: 1,
        }
    }

    #[test]
    fn put_get_flush_and_reload() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state.json");
        let id = SessionId("x//r".into());
        let uuid = "123e4567-e89b-42d3-a456-426614174000";
        let store = Store::load(p.clone());
        assert!(store.get(&id).is_none());
        store.put(&id, entry(uuid));
        assert!(!p.exists(), "writes are deferred until flush");
        store.flush();
        let again = Store::load(p);
        assert_eq!(again.get(&id), Some(entry(uuid)));
    }

    #[test]
    fn an_in_memory_store_remembers_but_never_writes() {
        let store = Store::in_memory();
        let id = SessionId("x//r".into());
        store.put(&id, entry("a"));
        store.flush();
        assert_eq!(store.get(&id), Some(entry("a")));
    }

    #[tokio::test]
    async fn the_writer_debounces_bursts_into_one_write() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state.json");
        let store = Store::load(p.clone());
        let writer = tokio::spawn(Arc::clone(&store).run_writer());
        let id = SessionId("x//r".into());
        let uuid = "123e4567-e89b-42d3-a456-426614174000";
        store.put(&id, entry(uuid));
        tokio::time::sleep(DEBOUNCE / 5).await;
        assert!(!p.exists(), "still inside the debounce window");
        tokio::time::sleep(DEBOUNCE * 3).await;
        assert_eq!(Store::load(p).get(&id), Some(entry(uuid)));
        writer.abort();
    }
}
