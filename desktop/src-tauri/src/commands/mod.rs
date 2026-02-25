pub mod inbox;
pub mod projects;
pub mod services;

use ipc::error::IpcError;
use serde::Serialize;

/// Serializable error wrapper that preserves categorical error codes.
/// Tauri requires command error types to implement `serde::Serialize`.
#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl From<canopus_inbox::error::InboxError> for CommandError {
    fn from(e: canopus_inbox::error::InboxError) -> Self {
        Self {
            code: e.code(),
            message: e.to_string(),
        }
    }
}

impl From<IpcError> for CommandError {
    fn from(e: IpcError) -> Self {
        Self {
            code: e.code(),
            message: e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipc::error::IpcError;

    #[test]
    fn ipc001_connection_failed_preserved() {
        let cmd: CommandError = IpcError::ConnectionFailed("refused".to_string()).into();
        assert_eq!(cmd.code, "IPC001");
        assert!(cmd.message.contains("refused"));
    }

    #[test]
    fn ipc008_timeout_serializes_correctly() {
        let cmd: CommandError = IpcError::Timeout("slow".to_string()).into();
        assert_eq!(cmd.code, "IPC008");
        let json = serde_json::to_string(&cmd).expect("CommandError must serialize");
        assert!(json.contains("\"code\":\"IPC008\""));
        assert!(json.contains("slow"));
    }

    #[test]
    fn ipc006_empty_response_has_no_payload() {
        let cmd: CommandError = IpcError::EmptyResponse.into();
        assert_eq!(cmd.code, "IPC006");
    }

    #[test]
    fn inbox_error_still_maps_via_command_error() {
        use canopus_inbox::error::InboxError;
        let cmd: CommandError = InboxError::NotFound("item-9".to_string()).into();
        assert_eq!(cmd.code, "INBOX001");
        assert!(cmd.message.contains("item-9"));
    }
}
