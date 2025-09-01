#[cfg(test)]
mod tests {
    use crate::{IpcError, Result};
    use std::error::Error;

    #[test]
    fn test_ipc_error_display() {
        let err = IpcError::ConnectionFailed("server unreachable".to_string());
        assert_eq!(err.to_string(), "Connection failed: server unreachable");

        let err = IpcError::SerializationFailed("json parse error".to_string());
        assert_eq!(err.to_string(), "Serialization failed: json parse error");

        let err = IpcError::ProtocolError("invalid message format".to_string());
        assert_eq!(err.to_string(), "Protocol error: invalid message format");

        let err = IpcError::Timeout("request timeout".to_string());
        assert_eq!(err.to_string(), "Timeout error: request timeout");

        let err = IpcError::DeserializationFailed("invalid token".to_string());
        assert_eq!(err.to_string(), "Deserialization failed: invalid token");
    }

    #[test]
    fn test_ipc_empty_response() {
        let err = IpcError::EmptyResponse;
        assert_eq!(err.to_string(), "Received empty response");
        assert_eq!(err.code(), "IPC006");
    }

    #[test]
    fn test_result_type_alias() {
        fn returns_success() -> Result<Vec<u8>> {
            Ok(vec![1, 2, 3])
        }

        fn returns_failure() -> Result<Vec<u8>> {
            Err(IpcError::ConnectionFailed("network down".to_string()))
        }

        assert!(returns_success().is_ok());
        assert!(returns_failure().is_err());
    }

    #[test]
    fn test_error_trait_implementation() {
        let err = IpcError::ProtocolError("test".to_string());

        // Test that it implements std::error::Error
        let _: &dyn Error = &err;

        // Test source method
        assert!(err.source().is_none());
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(
            IpcError::ConnectionFailed("test".to_string()).code(),
            "IPC001"
        );
        assert_eq!(IpcError::SendFailed("test".to_string()).code(), "IPC002");
        assert_eq!(IpcError::ReceiveFailed("test".to_string()).code(), "IPC003");
        assert_eq!(
            IpcError::SerializationFailed("test".to_string()).code(),
            "IPC004"
        );
        assert_eq!(
            IpcError::DeserializationFailed("test".to_string()).code(),
            "IPC005"
        );
        assert_eq!(IpcError::EmptyResponse.code(), "IPC006");
        assert_eq!(IpcError::ProtocolError("test".to_string()).code(), "IPC007");
        assert_eq!(IpcError::Timeout("test".to_string()).code(), "IPC008");
    }
}
