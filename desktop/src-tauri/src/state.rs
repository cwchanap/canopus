use canopus_inbox::store::SqliteStore;
use ipc::uds_client::JsonRpcClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

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
    pub ipc: JsonRpcClient,
    pub inbox: SqliteStore,
    pub projects_path: PathBuf,
    /// Active log-tail tasks keyed by service ID.
    pub log_tails: Mutex<HashMap<String, JoinHandle<()>>>,
}

impl AppState {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let socket_path = std::env::var("CANOPUS_IPC_SOCKET")
            .unwrap_or_else(|_| "/tmp/canopus.sock".to_string());
        let token = std::env::var("CANOPUS_IPC_TOKEN").ok();

        let ipc = JsonRpcClient::new(socket_path, token);
        let inbox = SqliteStore::open_default()?;

        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let projects_path = PathBuf::from(home).join(".canopus").join("projects.json");

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
        let tails_summary = match self.log_tails.try_lock() {
            Ok(guard) => format!("{} active", guard.len()),
            Err(_) => "<locked>".to_string(),
        };
        f.debug_struct("AppState")
            .field("projects_path", &self.projects_path)
            .field("ipc", &"<redacted ipc>")
            .field("inbox", &"<redacted inbox>")
            .field("log_tails", &tails_summary)
            .finish()
    }
}
