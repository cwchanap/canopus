#[cfg(test)]
mod tests {
    use crate::{DaemonError, Result};
    use std::error::Error;
    use std::io;

    #[test]
    fn test_daemon_error_display() {
        let err = DaemonError::ServerError("service init failed".to_string());
        assert_eq!(err.to_string(), "Server error: service init failed");

        let err = DaemonError::ConnectionError("bad daemon config".to_string());
        assert_eq!(err.to_string(), "Connection error: bad daemon config");

        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let err = DaemonError::IoError(io_err);
        assert!(err.to_string().contains("access denied"));

        let serde_err = serde_json::from_str::<serde_json::Value>("{invalid}").unwrap_err();
        let err = DaemonError::SerializationError(serde_err);
        assert!(err.to_string().contains("Serialization error"));
    }

    #[test]
    fn test_daemon_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let daemon_err: DaemonError = io_err.into();

        if let DaemonError::IoError(_) = daemon_err {
            // Expected variant
        } else {
            panic!("Expected DaemonError::IoError variant");
        }
    }

    #[test]
    fn test_daemon_error_from_serde_json() {
        let serde_err = serde_json::from_str::<serde_json::Value>("{invalid}").unwrap_err();
        let daemon_err: DaemonError = serde_err.into();

        if let DaemonError::SerializationError(_) = daemon_err {
            // Expected variant
        } else {
            panic!("Expected DaemonError::SerializationError variant");
        }
    }

    #[test]
    fn test_result_type_alias() {
        fn returns_ok() -> Result<u32> {
            Ok(42)
        }

        fn returns_err() -> Result<u32> {
            Err(DaemonError::ServerError("test failure".to_string()))
        }

        assert_eq!(returns_ok().unwrap(), 42);
        assert!(returns_err().is_err());
    }

    #[test]
    fn test_error_trait_implementation() {
        let err = DaemonError::ConnectionError("test".to_string());

        // Test that it implements std::error::Error
        let _: &dyn Error = &err;

        // Test source method
        assert!(err.source().is_none());
    }
}
