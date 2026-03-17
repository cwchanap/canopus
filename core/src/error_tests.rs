#[cfg(test)]
mod tests {
    use crate::error::*;
    use std::error::Error;
    use std::io;

    #[test]
    fn test_core_error_display() {
        let err = CoreError::ValidationError("test validation".to_string());
        assert_eq!(err.to_string(), "Validation error: test validation");

        let err = CoreError::ConfigurationError("bad config".to_string());
        assert_eq!(err.to_string(), "Configuration error: bad config");

        let err = CoreError::InitializationError("init failed".to_string());
        assert_eq!(err.to_string(), "Initialization error: init failed");

        let err = CoreError::ServiceError("service down".to_string());
        assert_eq!(err.to_string(), "Service error: service down");

        let err = CoreError::Other("generic error".to_string());
        assert_eq!(err.to_string(), "Generic error: generic error");
    }

    #[test]
    fn test_core_error_from_std_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let core_err: CoreError = io_err.into();

        if let CoreError::IoError(_) = core_err {
            // Expected variant
        } else {
            panic!("Expected CoreError::IoError variant");
        }
    }

    #[test]
    fn test_core_error_from_serde_error() {
        let serde_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let core_err: CoreError = serde_err.into();

        if let CoreError::SerializationError(_) = core_err {
            // Expected variant
        } else {
            panic!("Expected CoreError::SerializationError variant");
        }
    }

    #[test]
    fn test_result_type_alias() {
        #[allow(clippy::unnecessary_wraps)]
        fn returns_result() -> Result<String> {
            Ok("success".to_string())
        }

        fn returns_error() -> Result<String> {
            Err(CoreError::ValidationError("test".to_string()))
        }

        assert!(returns_result().is_ok());
        assert!(returns_error().is_err());
    }

    #[test]
    fn test_error_trait_implementation() {
        let err = CoreError::ValidationError("test".to_string());

        // Test that it implements std::error::Error
        let _: &dyn Error = &err;

        // Test source method (should return None for basic string errors)
        assert!(err.source().is_none());
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(
            CoreError::ConfigurationError("test".to_string()).code(),
            "CORE001"
        );
        assert_eq!(
            CoreError::ValidationError("test".to_string()).code(),
            "CORE002"
        );
        assert_eq!(
            CoreError::InitializationError("test".to_string()).code(),
            "CORE003"
        );
        assert_eq!(
            CoreError::ServiceError("test".to_string()).code(),
            "CORE004"
        );
        assert_eq!(CoreError::Other("test".to_string()).code(), "CORE999");
    }

    #[test]
    fn test_all_error_codes() {
        let io_err = io::Error::other("io");
        assert_eq!(CoreError::IoError(io_err).code(), "CORE005");

        let serde_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
        assert_eq!(CoreError::SerializationError(serde_err).code(), "CORE006");

        assert_eq!(CoreError::PortInUse(8080).code(), "CORE007");
        assert_eq!(CoreError::NoAvailablePort { tried: 5 }.code(), "CORE008");
        assert_eq!(CoreError::ProcessSpawn("x".to_string()).code(), "CORE009");
        assert_eq!(CoreError::ProcessSignal("x".to_string()).code(), "CORE010");
        assert_eq!(CoreError::ProcessWait("x".to_string()).code(), "CORE011");
    }

    #[test]
    fn test_remaining_error_display_variants() {
        assert_eq!(
            CoreError::PortInUse(9000).to_string(),
            "Port 9000 is already in use"
        );
        assert!(CoreError::NoAvailablePort { tried: 10 }
            .to_string()
            .contains("10"));
        assert!(CoreError::ProcessSpawn("spawn fail".to_string())
            .to_string()
            .contains("spawn fail"));
        assert!(CoreError::ProcessSignal("sig fail".to_string())
            .to_string()
            .contains("sig fail"));
        assert!(CoreError::ProcessWait("wait fail".to_string())
            .to_string()
            .contains("wait fail"));
    }
}
