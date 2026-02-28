use canopus_inbox::store::SqliteStore;
use ipc::uds_client::JsonRpcClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

type LogTailEntry = (u64, Option<JoinHandle<()>>);

/// A named group of Canopus services.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    pub service_ids: Vec<String>,
}

/// Top-level project config stored in ~/.canopus/projects.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub projects: Vec<Project>,
}

/// Shared application state managed by Tauri.
pub struct AppState {
    pub(crate) ipc: JsonRpcClient,
    pub(crate) inbox: SqliteStore,
    pub(crate) projects_path: PathBuf,
    /// Active log-tail tasks keyed by service ID.
    /// The u64 is a monotonically-increasing generation counter used to prevent
    /// a task that ends naturally from evicting a newer tail for the same service.
    /// The handle is `None` while `start_log_tail` is awaiting `tail_logs`; this
    /// placeholder lets `stop_log_tail` mark the entry as cancelled before the
    /// real `JoinHandle` exists, preventing orphaned background tasks.
    pub(crate) log_tails: Mutex<HashMap<String, LogTailEntry>>,
}

impl AppState {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let socket_path = std::env::var("CANOPUS_IPC_SOCKET").unwrap_or_else(|_| {
            #[cfg(unix)]
            {
                "/tmp/canopus.sock".to_owned()
            }
            #[cfg(not(unix))]
            {
                std::env::temp_dir()
                    .join("canopus.sock")
                    .to_string_lossy()
                    .into_owned()
            }
        });
        let token = std::env::var("CANOPUS_IPC_TOKEN").ok();

        let ipc = JsonRpcClient::new(socket_path, token);
        let inbox = SqliteStore::open_default()?;

        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_or_else(|_| std::env::temp_dir(), PathBuf::from);
        let projects_path = home.join(".canopus").join("projects.json");

        Ok(Self {
            ipc,
            inbox,
            projects_path,
            log_tails: Mutex::new(HashMap::new()),
        })
    }
}

impl fmt::Debug for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tails_summary = self.log_tails.try_lock().map_or_else(
            |_| "<locked>".to_string(),
            |guard| format!("{} active", guard.len()),
        );
        f.debug_struct("AppState")
            .field("projects_path", &self.projects_path)
            .field("ipc", &"<redacted ipc>")
            .field("inbox", &"<redacted inbox>")
            .field("log_tails", &tails_summary)
            .finish()
    }
}
