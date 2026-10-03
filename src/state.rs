use std::{collections::HashMap, path::PathBuf, sync::Arc};

use sqlx::SqlitePool;
use tokio::{
    sync::{Mutex, Notify, broadcast},
    task::JoinHandle,
};

use crate::config::Config;

/// Something changed for a user; the UI reloads its current view.
#[derive(Clone, Debug)]
pub struct Event {
    pub user_id: i64,
    pub kind: &'static str,
}

pub struct SyncHandle {
    pub task: JoinHandle<()>,
    pub wake: Arc<Notify>,
}

pub struct Inner {
    pub db: SqlitePool,
    pub config: Config,
    pub events: broadcast::Sender<Event>,
    pub sync: Mutex<HashMap<i64, SyncHandle>>,
}

#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

impl std::ops::Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl AppState {
    pub fn new(db: SqlitePool, config: Config) -> Self {
        let (events, _) = broadcast::channel(256);
        Self(Arc::new(Inner {
            db,
            config,
            events,
            sync: Mutex::new(HashMap::new()),
        }))
    }

    pub fn notify(&self, user_id: i64, kind: &'static str) {
        let _ = self.events.send(Event { user_id, kind });
    }

    /// Ask an account's sync task to run a cycle now.
    pub async fn wake_sync(&self, account_id: i64) {
        if let Some(handle) = self.sync.lock().await.get(&account_id) {
            handle.wake.notify_one();
        }
    }

    pub fn raw_path(&self, account_id: i64, message_id: i64) -> PathBuf {
        self.config
            .data_dir
            .join("mail")
            .join(account_id.to_string())
            .join(format!("{message_id}.eml"))
    }

    pub fn draft_dir(&self, draft_id: i64) -> PathBuf {
        self.config.data_dir.join("drafts").join(draft_id.to_string())
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
