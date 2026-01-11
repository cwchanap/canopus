//! Inbox error types with categorical error codes.

use thiserror::Error;

/// Inbox-specific errors with categorical codes.
#[derive(Debug, Error)]
pub enum InboxError {
    /// Item not found (INBOX001)
    #[error("Inbox item not found: {0}")]
    NotFound(String),

    /// Database error (INBOX002)
    #[error("Database error: {0}")]
    Database(String),

    /// Serialization error (INBOX003)
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Notification error (INBOX004)
    #[error("Notification error: {0}")]
    Notification(String),

    /// Invalid input (INBOX005)
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// IO error (INBOX006)
    #[error("IO error: {0}")]
    Io(String),
}

impl InboxError {
    /// Returns the categorical error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "INBOX001",
            Self::Database(_) => "INBOX002",
            Self::Serialization(_) => "INBOX003",
            Self::Notification(_) => "INBOX004",
            Self::InvalidInput(_) => "INBOX005",
            Self::Io(_) => "INBOX006",
        }
    }
}

impl From<rusqlite::Error> for InboxError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Database(err.to_string())
    }
}

impl From<serde_json::Error> for InboxError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

impl From<std::io::Error> for InboxError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

/// Result type alias for inbox operations.
pub type Result<T> = std::result::Result<T, InboxError>;
