use canopus_inbox::item::{InboxFilter, InboxItem, InboxStatus, SourceAgent};
use canopus_inbox::store::InboxStore;
use serde::Deserialize;
use tauri::State;

use crate::state::AppState;

/// Input filter from the frontend (string-based for JSON serialization).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxFilterInput {
    pub status: Option<String>,
    pub source_agent: Option<String>,
    pub project: Option<String>,
    pub limit: Option<u32>,
}

impl From<InboxFilterInput> for InboxFilter {
    fn from(input: InboxFilterInput) -> Self {
        Self {
            status: input.status.as_deref().map(InboxStatus::from_str),
            source_agent: input.source_agent.as_deref().map(SourceAgent::from_str),
            project: input.project,
            limit: input.limit,
        }
    }
}

#[tauri::command]
pub async fn list_inbox(
    state: State<'_, AppState>,
    filter: Option<InboxFilterInput>,
) -> Result<Vec<InboxItem>, String> {
    let f = filter.map(InboxFilter::from).unwrap_or_default();
    state.inbox.list(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_inbox_read(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    state.inbox.mark_read(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dismiss_inbox_item(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    state.inbox.dismiss(&id).await.map_err(|e| e.to_string())
}
