//! Schema definitions for Canopus
//!
//! This crate contains shared data structures and schemas used across
//! the entire Canopus ecosystem. All types here implement JSON Schema
//! generation for external consumption.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Message types for communication between daemon and CLI
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Message {
    /// Request daemon status
    Status,
    /// Start daemon operations
    Start,
    /// Stop daemon operations
    Stop,
    /// Restart daemon
    Restart,
    /// Send custom command with payload
    Custom { 
        #[serde(rename = "command")]
        cmd: String 
    },
}

/// Response types from the daemon
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Response {
    /// Successful operation with message
    Ok { message: String },
    /// Error response with details
    Error { 
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    /// Status information
    Status { 
        running: bool, 
        uptime_seconds: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
}

/// Configuration structure for the daemon
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonConfig {
    /// Host to bind the daemon to
    pub host: String,
    /// Port to bind the daemon to
    pub port: u16,
    /// Log level for the daemon
    pub log_level: String,
    /// Maximum number of concurrent connections
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            log_level: "info".to_string(),
            max_connections: default_max_connections(),
        }
    }
}

fn default_max_connections() -> usize {
    100
}

/// Client configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfig {
    /// Daemon host to connect to
    pub daemon_host: String,
    /// Daemon port to connect to
    pub daemon_port: u16,
    /// Connection timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            daemon_host: "127.0.0.1".to_string(),
            daemon_port: 8080,
            timeout_seconds: default_timeout(),
        }
    }
}

fn default_timeout() -> u64 {
    30
}

/// Event types that can be emitted by the system
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Event {
    /// Daemon started
    DaemonStarted { 
        timestamp: String, 
        version: String 
    },
    /// Daemon stopped
    DaemonStopped { 
        timestamp: String 
    },
    /// Client connected
    ClientConnected { 
        timestamp: String, 
        client_id: String 
    },
    /// Client disconnected
    ClientDisconnected { 
        timestamp: String, 
        client_id: String 
    },
    /// Custom event
    Custom { 
        event_type: String, 
        timestamp: String, 
        data: serde_json::Value 
    },
}

/// System state representation
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SystemState {
    /// Whether the daemon is running
    pub daemon_running: bool,
    /// Number of active connections
    pub active_connections: usize,
    /// System uptime in seconds
    pub uptime_seconds: u64,
    /// Current system version
    pub version: String,
    /// Last update timestamp
    pub last_updated: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::schema_for;
    
    #[test]
    fn test_message_serialization() {
        let msg = Message::Custom { cmd: "test".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("test"));
    }
    
    #[test]
    fn test_response_serialization() {
        let resp = Response::Ok { message: "success".to_string() };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("success"));
    }
    
    #[test]
    fn test_schema_generation() {
        // Just check that schemas can be generated without panicking
        let _message_schema = schema_for!(Message);
        let _response_schema = schema_for!(Response);
        let _daemon_config_schema = schema_for!(DaemonConfig);
        let _client_config_schema = schema_for!(ClientConfig);
        
        // Test passes if we get here without panicking
        assert!(true);
    }
    
    #[test]
    fn test_default_configs() {
        let daemon_config = DaemonConfig::default();
        assert_eq!(daemon_config.host, "127.0.0.1");
        assert_eq!(daemon_config.port, 8080);
        
        let client_config = ClientConfig::default();
        assert_eq!(client_config.daemon_host, "127.0.0.1");
        assert_eq!(client_config.daemon_port, 8080);
    }
}
