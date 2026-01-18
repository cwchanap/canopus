//! Inbox item types and related structures.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Source AI agent that created the inbox item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SourceAgent {
    /// Claude Code by Anthropic
    ClaudeCode,
    /// `OpenAI` Codex CLI
    Codex,
    /// Windsurf IDE
    Windsurf,
    /// `OpenCode` AI CLI
    OpenCode,
    /// Other/unknown agent
    Other,
}

impl SourceAgent {
    /// Returns the display name for the agent.
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex CLI",
            Self::Windsurf => "Windsurf",
            Self::OpenCode => "OpenCode",
            Self::Other => "Other",
        }
    }

    /// Parses a source agent from a string.
    ///
    /// Accepts various formats:
    /// - `"claude-code"`, `"claudecode"`, `"claude"` → `ClaudeCode`
    /// - `"codex"`, `"codex-cli"` → `Codex`
    /// - `"windsurf"` → `Windsurf`
    /// - `"opencode"`, `"open-code"` → `OpenCode`
    /// - Any other value → `Other`
    #[must_use]
    #[allow(clippy::should_implement_trait)] // Infallible parsing, not FromStr
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "claude-code" | "claudecode" | "claude" => Self::ClaudeCode,
            "codex" | "codex-cli" => Self::Codex,
            "windsurf" => Self::Windsurf,
            "opencode" | "open-code" => Self::OpenCode,
            _ => Self::Other,
        }
    }

    /// Returns the string representation for storage.
    ///
    /// This is the canonical kebab-case form used in the database and JSON.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Windsurf => "windsurf",
            Self::OpenCode => "opencode",
            Self::Other => "other",
        }
    }
}

impl std::fmt::Display for SourceAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Status of an inbox item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum InboxStatus {
    /// Item has not been read yet.
    #[default]
    Unread,
    /// Item has been read but not dismissed.
    Read,
    /// Item has been dismissed/archived.
    Dismissed,
}

impl InboxStatus {
    /// Parses a status from a string.
    ///
    /// Accepts case-insensitive values:
    /// - `"read"` → `Read`
    /// - `"dismissed"` → `Dismissed`
    /// - Any other value → `Unread` (default)
    #[must_use]
    #[allow(clippy::should_implement_trait)] // Infallible parsing, not FromStr
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "read" => Self::Read,
            "dismissed" => Self::Dismissed,
            _ => Self::Unread,
        }
    }

    /// Returns the string representation for storage.
    ///
    /// This is the canonical lowercase form used in the database and JSON.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Unread => "unread",
            Self::Read => "read",
            Self::Dismissed => "dismissed",
        }
    }
}

impl std::fmt::Display for InboxStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Data for creating a new inbox item.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewInboxItem {
    /// Name of the project this notification is about.
    pub project_name: String,
    /// Brief summary of the current status.
    pub status_summary: String,
    /// Description of what action the user needs to take.
    pub action_required: String,
    /// Which AI agent generated this notification.
    pub source_agent: SourceAgent,
    /// Optional additional context data (JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl NewInboxItem {
    /// Creates a new inbox item.
    #[must_use]
    pub fn new(
        project_name: impl Into<String>,
        status_summary: impl Into<String>,
        action_required: impl Into<String>,
        source_agent: SourceAgent,
    ) -> Self {
        Self {
            project_name: project_name.into(),
            status_summary: status_summary.into(),
            action_required: action_required.into(),
            source_agent,
            details: None,
        }
    }

    /// Adds optional details.
    #[must_use]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// A complete inbox item with all metadata.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    /// Unique identifier for the item.
    pub id: String,
    /// Name of the project this notification is about.
    pub project_name: String,
    /// Brief summary of the current status.
    pub status_summary: String,
    /// Description of what action the user needs to take.
    pub action_required: String,
    /// Which AI agent generated this notification.
    pub source_agent: SourceAgent,
    /// Optional additional context data (JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    /// Current status of the item.
    pub status: InboxStatus,
    /// When the item was created.
    pub created_at: DateTime<Utc>,
    /// When the item was last updated.
    pub updated_at: DateTime<Utc>,
    /// When the item was dismissed (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed_at: Option<DateTime<Utc>>,
    /// Whether a notification was sent for this item.
    pub notified: bool,
}

impl InboxItem {
    /// Creates a new inbox item from creation data.
    #[must_use]
    pub fn from_new(new_item: NewInboxItem) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            project_name: new_item.project_name,
            status_summary: new_item.status_summary,
            action_required: new_item.action_required,
            source_agent: new_item.source_agent,
            details: new_item.details,
            status: InboxStatus::Unread,
            created_at: now,
            updated_at: now,
            dismissed_at: None,
            notified: false,
        }
    }

    /// Returns the age of this item as a human-readable string.
    #[must_use]
    pub fn age_display(&self) -> String {
        let duration = Utc::now().signed_duration_since(self.created_at);

        if duration.num_days() > 0 {
            format!("{}d ago", duration.num_days())
        } else if duration.num_hours() > 0 {
            format!("{}h ago", duration.num_hours())
        } else if duration.num_minutes() > 0 {
            format!("{}m ago", duration.num_minutes())
        } else {
            "just now".to_string()
        }
    }
}

/// Query filter for listing inbox items.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InboxFilter {
    /// Filter by status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<InboxStatus>,
    /// Filter by source agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_agent: Option<SourceAgent>,
    /// Filter by project name (partial match).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Maximum number of items to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}
