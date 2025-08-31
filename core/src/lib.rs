//! Core functionality for the Canopus project
//! 
//! This crate contains shared types, utilities, and business logic
//! that can be used by both the daemon and CLI components.

use serde::{Deserialize, Serialize};

/// Configuration structure for the Canopus system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub daemon_host: String,
    pub daemon_port: u16,
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            daemon_host: "127.0.0.1".to_string(),
            daemon_port: 8080,
            log_level: "info".to_string(),
        }
    }
}

/// Common result type used throughout the application
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Message types for communication between daemon and CLI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Status,
    Start,
    Stop,
    Restart,
    Custom(String),
}

/// Response types from the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Ok(String),
    Error(String),
    Status { running: bool, uptime: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.daemon_host, "127.0.0.1");
        assert_eq!(config.daemon_port, 8080);
        assert_eq!(config.log_level, "info");
    }
}
