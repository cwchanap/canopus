//! Schema definitions for Canopus
//!
//! This crate contains shared data structures and schemas used across
//! the entire Canopus ecosystem. All types here implement JSON Schema
//! generation for external consumption.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Service management types
pub mod events;
pub mod service;

// Testing modules
#[cfg(test)]
mod json_roundtrip_tests;

// Re-export service types for convenience
pub use events::*;
pub use service::*;

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
        /// The custom command to execute
        cmd: String,
    },
}

/// Response types from the daemon
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Response {
    /// Successful operation with message
    Ok {
        /// Success message
        message: String,
    },
    /// Error response with details
    Error {
        /// Error message
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        /// Optional error code
        code: Option<String>,
    },
    /// Status information
    Status {
        /// Whether the daemon is running
        running: bool,
        /// Uptime in seconds
        uptime_seconds: u64,
        /// Process ID of the daemon
        pid: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        /// Optional version string
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
            port: 49384,
            log_level: "info".to_string(),
            max_connections: default_max_connections(),
        }
    }
}

const fn default_max_connections() -> usize {
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
            daemon_port: 49384,
            timeout_seconds: default_timeout(),
        }
    }
}

const fn default_timeout() -> u64 {
    30
}

/// Event types that can be emitted by the system
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Event {
    /// Daemon started
    DaemonStarted {
        /// Event timestamp
        timestamp: String,
        /// Daemon version
        version: String,
    },
    /// Daemon stopped
    DaemonStopped {
        /// Event timestamp
        timestamp: String,
    },
    /// Client connected
    ClientConnected {
        /// Event timestamp
        timestamp: String,
        /// Client identifier
        client_id: String,
    },
    /// Client disconnected
    ClientDisconnected {
        /// Event timestamp
        timestamp: String,
        /// Client identifier
        client_id: String,
    },
    /// Custom event
    Custom {
        /// Event type identifier
        event_type: String,
        /// Event timestamp
        timestamp: String,
        /// Event data payload
        data: serde_json::Value,
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
        let msg = Message::Custom {
            cmd: "test".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("test"));
    }

    #[test]
    fn test_response_serialization() {
        let resp = Response::Ok {
            message: "success".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("success"));
    }

    #[test]
    fn test_schema_generation() {
        // Test that schemas can be generated successfully
        let message_schema = schema_for!(Message);
        let response_schema = schema_for!(Response);
        let daemon_config_schema = schema_for!(DaemonConfig);
        let client_config_schema = schema_for!(ClientConfig);

        // Verify schemas have the expected structure
        assert!(message_schema.schema.metadata.is_some());
        assert!(response_schema.schema.metadata.is_some());
        assert!(daemon_config_schema.schema.metadata.is_some());
        assert!(client_config_schema.schema.metadata.is_some());
    }

    #[test]
    fn test_default_configs() {
        let daemon_config = DaemonConfig::default();
        assert_eq!(daemon_config.host, "127.0.0.1");
        assert_eq!(daemon_config.port, 49384);

        let client_config = ClientConfig::default();
        assert_eq!(client_config.daemon_host, "127.0.0.1");
        assert_eq!(client_config.daemon_port, 49384);
    }

    #[test]
    fn test_daemon_config_defaults() {
        let cfg = DaemonConfig::default();
        assert_eq!(cfg.log_level, "info");
        assert_eq!(cfg.max_connections, 100);
    }

    #[test]
    fn test_client_config_default_timeout() {
        let cfg = ClientConfig::default();
        assert_eq!(cfg.timeout_seconds, 30);
    }

    #[test]
    fn test_message_all_variants_serialize() {
        let msgs = vec![
            Message::Status,
            Message::Start,
            Message::Stop,
            Message::Restart,
            Message::Custom {
                cmd: "do_thing".to_string(),
            },
        ];
        for msg in &msgs {
            let json = serde_json::to_string(msg).expect("serialize");
            let back: Message = serde_json::from_str(&json).expect("deserialize");
            // Check round-trip by re-serializing
            let json2 = serde_json::to_string(&back).expect("re-serialize");
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn test_response_all_variants_serialize() {
        let resps = vec![
            Response::Ok {
                message: "done".to_string(),
            },
            Response::Error {
                message: "bad".to_string(),
                code: Some("ERR001".to_string()),
            },
            Response::Error {
                message: "no code".to_string(),
                code: None,
            },
            Response::Status {
                running: true,
                uptime_seconds: 100,
                pid: 1234,
                version: Some("1.0.0".to_string()),
            },
            Response::Status {
                running: false,
                uptime_seconds: 0,
                pid: 0,
                version: None,
            },
        ];
        for resp in &resps {
            let json = serde_json::to_string(resp).expect("serialize");
            let back: Response = serde_json::from_str(&json).expect("deserialize");
            let json2 = serde_json::to_string(&back).expect("re-serialize");
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn test_response_error_code_omitted_when_none() {
        let resp = Response::Error {
            message: "oops".to_string(),
            code: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        // code field should be absent (skip_serializing_if = "Option::is_none")
        assert!(
            !json.contains("\"code\""),
            "code field should be omitted when None, got: {json}"
        );
    }

    #[test]
    fn test_response_status_version_omitted_when_none() {
        let resp = Response::Status {
            running: false,
            uptime_seconds: 0,
            pid: 0,
            version: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(
            !json.contains("\"version\""),
            "version field should be omitted when None, got: {json}"
        );
    }

    #[test]
    fn test_system_state_roundtrip() {
        let state = SystemState {
            daemon_running: true,
            active_connections: 5,
            uptime_seconds: 3600,
            version: "2.0.0".to_string(),
            last_updated: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: SystemState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.daemon_running, state.daemon_running);
        assert_eq!(back.active_connections, state.active_connections);
        assert_eq!(back.uptime_seconds, state.uptime_seconds);
        assert_eq!(back.version, state.version);
        assert_eq!(back.last_updated, state.last_updated);
    }

    #[test]
    fn test_event_all_variants_serialize() {
        let events = vec![
            Event::DaemonStarted {
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                version: "1.0.0".to_string(),
            },
            Event::DaemonStopped {
                timestamp: "2024-01-01T00:00:01Z".to_string(),
            },
            Event::ClientConnected {
                timestamp: "2024-01-01T00:00:02Z".to_string(),
                client_id: "client-1".to_string(),
            },
            Event::ClientDisconnected {
                timestamp: "2024-01-01T00:00:03Z".to_string(),
                client_id: "client-1".to_string(),
            },
            Event::Custom {
                event_type: "my_event".to_string(),
                timestamp: "2024-01-01T00:00:04Z".to_string(),
                data: serde_json::json!({"key": "value"}),
            },
        ];
        for event in &events {
            let json = serde_json::to_string(event).expect("serialize");
            let back: Event = serde_json::from_str(&json).expect("deserialize");
            let json2 = serde_json::to_string(&back).expect("re-serialize");
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn test_message_camel_case_serialization() {
        // Custom variant field should be "command" due to rename
        let msg = Message::Custom {
            cmd: "hello".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains("\"command\""),
            "cmd field renamed to 'command', got: {json}"
        );
    }

    #[test]
    fn test_daemon_config_serialization_camel_case() {
        let cfg = DaemonConfig {
            host: "0.0.0.0".to_string(),
            port: 8080,
            log_level: "debug".to_string(),
            max_connections: 50,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"logLevel\""), "got: {json}");
        assert!(json.contains("\"maxConnections\""), "got: {json}");
    }
}
