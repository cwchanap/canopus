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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_for_all_variants() {
        assert_eq!(InboxError::NotFound("x".to_string()).code(), "INBOX001");
        assert_eq!(InboxError::Database("x".to_string()).code(), "INBOX002");
        assert_eq!(InboxError::Serialization("x".to_string()).code(), "INBOX003");
        assert_eq!(InboxError::Notification("x".to_string()).code(), "INBOX004");
        assert_eq!(InboxError::InvalidInput("x".to_string()).code(), "INBOX005");
        assert_eq!(InboxError::Io("x".to_string()).code(), "INBOX006");
    }

    #[test]
    fn display_messages_for_all_variants() {
        let cases = [
            (
                InboxError::NotFound("item-1".to_string()),
                "Inbox item not found: item-1",
            ),
            (
                InboxError::Database("db down".to_string()),
                "Database error: db down",
            ),
            (
                InboxError::Serialization("bad json".to_string()),
                "Serialization error: bad json",
            ),
            (
                InboxError::Notification("no notify".to_string()),
                "Notification error: no notify",
            ),
            (
                InboxError::InvalidInput("bad input".to_string()),
                "Invalid input: bad input",
            ),
            (
                InboxError::Io("io fail".to_string()),
                "IO error: io fail",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(
                err.to_string(),
                expected,
                "wrong display message for variant with expected '{expected}'"
            );
        }
    }

    #[test]
    fn from_serde_json_error_creates_serialization_variant() {
        let serde_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let inbox_err = InboxError::from(serde_err);
        assert!(
            matches!(inbox_err, InboxError::Serialization(_)),
            "expected Serialization variant, got {inbox_err:?}"
        );
        assert_eq!(inbox_err.code(), "INBOX003");
    }

    #[test]
    fn from_io_error_creates_io_variant() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let inbox_err = InboxError::from(io_err);
        assert!(
            matches!(inbox_err, InboxError::Io(_)),
            "expected Io variant, got {inbox_err:?}"
        );
        assert_eq!(inbox_err.code(), "INBOX006");
        assert!(inbox_err.to_string().contains("IO error"), "got: {inbox_err}");
    }

    #[test]
    fn result_type_alias_ok_and_err() {
        let ok: Result<i32> = Ok(42);
        assert_eq!(ok.unwrap(), 42);
        let err: Result<i32> = Err(InboxError::NotFound("item".to_string()));
        assert!(err.is_err());
    }
}
