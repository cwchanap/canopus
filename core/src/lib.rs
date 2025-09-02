//! Core functionality for the Canopus project
//!
//! This crate contains shared types, utilities, and business logic
//! that can be used by both the daemon and CLI components.

pub mod error;
pub mod health;
pub mod port;
#[cfg(unix)]
pub mod process;
pub mod proxy_api;
pub mod supervisor;
pub mod utilities;

#[cfg(test)]
mod error_tests;

// Re-export schema types for convenience
pub use schema::*;

pub use error::{CoreError, Result};
pub use port::{PortAllocator, PortGuard};
pub use proxy_api::{CallLog, NullProxy, ProxyApi};
pub use utilities::*;

/// Core utilities and helper functions
pub mod utils {
    use tracing::{debug, info};

    /// Initialize tracing for the application
    pub fn init_tracing(level: &str) -> crate::Result<()> {
        use tracing_subscriber::{fmt, EnvFilter};

        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

        fmt()
            .with_env_filter(filter)
            .try_init()
            .map_err(|e| crate::CoreError::InitializationError(e.to_string()))?;

        info!("Tracing initialized with level: {}", level);
        Ok(())
    }

    /// Validate configuration values
    pub fn validate_config(config: &crate::DaemonConfig) -> crate::Result<()> {
        if config.port == 0 {
            return Err(crate::CoreError::ConfigurationError(
                "Port cannot be 0".to_string(),
            ));
        }

        if config.host.is_empty() {
            return Err(crate::CoreError::ConfigurationError(
                "Host cannot be empty".to_string(),
            ));
        }

        if config.max_connections == 0 {
            return Err(crate::CoreError::ConfigurationError(
                "Max connections must be greater than 0".to_string(),
            ));
        }

        debug!("Configuration validated successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_config() {
        let mut config = DaemonConfig::default();
        assert!(utils::validate_config(&config).is_ok());

        config.port = 0;
        assert!(utils::validate_config(&config).is_err());

        config.port = 8080;
        config.host = "".to_string();
        assert!(utils::validate_config(&config).is_err());

        config.host = "localhost".to_string();
        config.max_connections = 0;
        assert!(utils::validate_config(&config).is_err());
    }
}
