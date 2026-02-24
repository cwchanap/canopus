use canopus_inbox::error::InboxError;
use canopus_inbox::item::{InboxFilter, InboxItem, InboxStatus, SourceAgent};
use canopus_inbox::store::InboxStore;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

/// Serializable error wrapper that preserves the InboxError categorical code.
/// Tauri commands require the error type to implement `serde::Serialize`.
#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl From<InboxError> for CommandError {
    fn from(e: InboxError) -> Self {
        Self {
            code: e.code(),
            message: e.to_string(),
        }
    }
}

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
) -> Result<Vec<InboxItem>, CommandError> {
    let f = filter.map(InboxFilter::from).unwrap_or_default();
    state.inbox.list(f).await.map_err(CommandError::from)
}

#[tauri::command]
pub async fn mark_inbox_read(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    state.inbox.mark_read(&id).await.map_err(CommandError::from)
}

#[tauri::command]
pub async fn dismiss_inbox_item(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    state.inbox.dismiss(&id).await.map_err(CommandError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopus_inbox::error::InboxError;

    // ── Filter parsing tests ──────────────────────────────────────────────────

    #[test]
    fn filter_input_with_all_none_produces_default() {
        let input = InboxFilterInput {
            status: None,
            source_agent: None,
            project: None,
            limit: None,
        };
        let filter = InboxFilter::from(input);
        assert!(filter.status.is_none());
        assert!(filter.source_agent.is_none());
        assert!(filter.project.is_none());
        assert!(filter.limit.is_none());
    }

    #[test]
    fn filter_input_known_status_parsed() {
        let input = InboxFilterInput {
            status: Some("unread".to_string()),
            source_agent: None,
            project: None,
            limit: None,
        };
        let filter = InboxFilter::from(input);
        assert!(matches!(filter.status, Some(InboxStatus::Unread)));
    }

    #[test]
    fn filter_input_known_agent_parsed() {
        let input = InboxFilterInput {
            status: None,
            source_agent: Some("claudeCode".to_string()),
            project: None,
            limit: None,
        };
        let filter = InboxFilter::from(input);
        assert!(matches!(filter.source_agent, Some(SourceAgent::ClaudeCode)));
    }

    #[test]
    fn filter_input_unknown_values_use_defaults() {
        let input = InboxFilterInput {
            status: Some("bogus".to_string()),
            source_agent: Some("unknown_agent".to_string()),
            project: None,
            limit: None,
        };
        // Should not panic; InboxStatus::from_str and SourceAgent::from_str
        // fall back for unknown values.
        let _ = InboxFilter::from(input);
    }

    // ── CommandError / error code preservation tests ─────────────────────────

    #[test]
    fn command_error_preserves_inbox001_code() {
        let err = InboxError::NotFound("item-42".to_string());
        let cmd_err = CommandError::from(err);
        assert_eq!(cmd_err.code, "INBOX001");
        assert!(cmd_err.message.contains("item-42"));
    }

    #[test]
    fn command_error_preserves_inbox002_code() {
        let err = InboxError::Database("db failure".to_string());
        let cmd_err = CommandError::from(err);
        assert_eq!(cmd_err.code, "INBOX002");
    }

    #[test]
    fn command_error_is_serializable() {
        let err = InboxError::InvalidInput("bad input".to_string());
        let cmd_err = CommandError::from(err);
        let json = serde_json::to_string(&cmd_err).expect("CommandError must serialize");
        assert!(json.contains("\"code\":\"INBOX005\""));
        assert!(json.contains("bad input"));
    }
}
