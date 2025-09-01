//! Service specification and state management types for the Canopus supervisor
//!
//! This module contains the core data structures for defining and managing
//! service specifications, state transitions, restart policies, and health checks
//! in the Canopus supervisor system.
//!
//! ## Service Lifecycle
//!
//! Services progress through the following states:
//! - `Idle`: Service is not running
//! - `Spawning`: Service process is being started
//! - `Starting`: Service is running but not yet ready (health checks pending)
//! - `Ready`: Service is running and healthy
//! - `Stopping`: Service is being gracefully terminated
//!
//! ## Restart Policies
//!
//! The supervisor supports three restart policies:
//! - `Always`: Restart the service on any exit
//! - `OnFailure`: Only restart on non-zero exit codes
//! - `Never`: Never automatically restart the service

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Complete specification for a managed service
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSpec {
    /// Unique identifier for this service
    pub id: String,
    
    /// Human-readable name for the service
    pub name: String,
    
    /// Command to execute
    pub command: String,
    
    /// Command-line arguments
    #[serde(default)]
    pub args: Vec<String>,
    
    /// Environment variables to set for the process
    #[serde(default)]
    pub environment: HashMap<String, String>,
    
    /// Working directory for the process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    
    /// Restart policy for this service
    #[serde(default)]
    pub restart_policy: RestartPolicy,
    
    /// Backoff configuration for restart delays
    #[serde(default)]
    pub backoff_config: BackoffConfig,
    
    /// Health check configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<HealthCheck>,
    
    /// Readiness check configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness_check: Option<ReadinessCheck>,
    
    /// Maximum time to wait for graceful shutdown before using SIGKILL
    #[serde(default = "default_graceful_timeout_secs")]
    pub graceful_timeout_secs: u64,
    
    /// Maximum time to wait for the service to become ready after starting
    #[serde(default = "default_startup_timeout_secs")]
    pub startup_timeout_secs: u64,
}

impl ServiceSpec {
    /// Get the graceful timeout as a Duration
    pub fn graceful_timeout(&self) -> Duration {
        Duration::from_secs(self.graceful_timeout_secs)
    }
    
    /// Get the startup timeout as a Duration
    pub fn startup_timeout(&self) -> Duration {
        Duration::from_secs(self.startup_timeout_secs)
    }
}

const fn default_graceful_timeout_secs() -> u64 {
    30
}

const fn default_startup_timeout_secs() -> u64 {
    60
}

/// Current state of a managed service
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    /// Service is not running
    Idle,
    /// Service process is being started
    Spawning,
    /// Service is running but not yet ready (health checks pending)
    Starting,
    /// Service is running and healthy
    Ready,
    /// Service is being gracefully terminated
    Stopping,
}

impl ServiceState {
    /// Check if the service is in a running state (not Idle)
    pub fn is_running(&self) -> bool {
        !matches!(self, ServiceState::Idle)
    }
    
    /// Check if the service is ready to handle requests
    pub fn is_ready(&self) -> bool {
        matches!(self, ServiceState::Ready)
    }
    
    /// Check if the service is transitioning between states
    pub fn is_transitional(&self) -> bool {
        matches!(self, ServiceState::Spawning | ServiceState::Starting | ServiceState::Stopping)
    }
}

/// Restart policy determining when a service should be restarted
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RestartPolicy {
    /// Always restart the service when it exits
    Always,
    /// Only restart the service if it exits with a non-zero code
    OnFailure,
    /// Never automatically restart the service
    Never,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy::Always
    }
}

/// Configuration for exponential backoff restart delays
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackoffConfig {
    /// Base delay in seconds for the first restart attempt
    #[serde(default = "default_base_delay_secs")]
    pub base_delay_secs: u64,
    
    /// Multiplicative factor for exponential backoff
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,
    
    /// Maximum delay in seconds (caps exponential growth)
    #[serde(default = "default_max_delay_secs")]
    pub max_delay_secs: u64,
    
    /// Jitter percentage (0.0 to 1.0) to add randomness
    #[serde(default = "default_jitter")]
    pub jitter: f64,
    
    /// Window in seconds for counting failures (failures outside this window are ignored)
    #[serde(default = "default_failure_window_secs")]
    pub failure_window_secs: u64,
}

impl BackoffConfig {
    /// Get the base delay as a Duration
    pub fn base_delay(&self) -> Duration {
        Duration::from_secs(self.base_delay_secs)
    }
    
    /// Get the maximum delay as a Duration
    pub fn max_delay(&self) -> Duration {
        Duration::from_secs(self.max_delay_secs)
    }
    
    /// Get the failure window as a Duration
    pub fn failure_window(&self) -> Duration {
        Duration::from_secs(self.failure_window_secs)
    }
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            base_delay_secs: default_base_delay_secs(),
            multiplier: default_multiplier(),
            max_delay_secs: default_max_delay_secs(),
            jitter: default_jitter(),
            failure_window_secs: default_failure_window_secs(),
        }
    }
}

const fn default_base_delay_secs() -> u64 {
    1
}

const fn default_multiplier() -> f64 {
    2.0
}

const fn default_max_delay_secs() -> u64 {
    300 // 5 minutes
}

const fn default_jitter() -> f64 {
    0.1 // 10%
}

const fn default_failure_window_secs() -> u64 {
    900 // 15 minutes
}

/// Health check configuration for determining service health
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    /// Type of health check to perform
    pub check_type: HealthCheckType,
    
    /// Interval between health checks in seconds
    #[serde(default = "default_health_interval_secs")]
    pub interval_secs: u64,
    
    /// Timeout for each health check in seconds
    #[serde(default = "default_health_timeout_secs")]
    pub timeout_secs: u64,
    
    /// Number of consecutive failures before marking as unhealthy
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    
    /// Number of consecutive successes to mark as healthy again
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,
}

impl HealthCheck {
    /// Get the interval as a Duration
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.interval_secs)
    }
    
    /// Get the timeout as a Duration
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

/// Readiness check configuration for determining when service is ready
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessCheck {
    /// Type of readiness check to perform
    pub check_type: HealthCheckType,
    
    /// Initial delay before starting readiness checks in seconds
    #[serde(default = "default_initial_delay_secs")]
    pub initial_delay_secs: u64,
    
    /// Interval between readiness checks in seconds
    #[serde(default = "default_readiness_interval_secs")]
    pub interval_secs: u64,
    
    /// Timeout for each readiness check in seconds
    #[serde(default = "default_readiness_timeout_secs")]
    pub timeout_secs: u64,
    
    /// Number of consecutive successes required to mark as ready
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,
}

impl ReadinessCheck {
    /// Get the initial delay as a Duration
    pub fn initial_delay(&self) -> Duration {
        Duration::from_secs(self.initial_delay_secs)
    }
    
    /// Get the interval as a Duration
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.interval_secs)
    }
    
    /// Get the timeout as a Duration
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

/// Type of health/readiness check to perform
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HealthCheckType {
    /// TCP connection check to a specific port
    Tcp {
        /// Port to connect to
        port: u16,
    },
    /// HTTP GET request to a specific path
    Http {
        /// Port for HTTP request
        port: u16,
        /// Path to request (default: "/health")
        #[serde(default = "default_health_path")]
        path: String,
        /// Expected HTTP status codes (default: [200])
        #[serde(default = "default_success_codes")]
        success_codes: Vec<u16>,
    },
    /// Execute a command and check exit code
    Exec {
        /// Command to execute
        command: String,
        /// Arguments for the command
        #[serde(default)]
        args: Vec<String>,
    },
}

fn default_health_path() -> String {
    "/health".to_string()
}

fn default_success_codes() -> Vec<u16> {
    vec![200]
}

const fn default_health_interval_secs() -> u64 {
    30
}

const fn default_health_timeout_secs() -> u64 {
    5
}

const fn default_readiness_interval_secs() -> u64 {
    5
}

const fn default_readiness_timeout_secs() -> u64 {
    3
}

const fn default_initial_delay_secs() -> u64 {
    0
}

const fn default_failure_threshold() -> u32 {
    3
}

const fn default_success_threshold() -> u32 {
    1
}

/// Information about a service process exit
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceExit {
    /// Process ID that exited
    pub pid: u32,
    
    /// Exit code (None if killed by signal)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    
    /// Signal that killed the process (Unix only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    
    /// Timestamp when the exit was detected
    pub timestamp: String,
}

impl ServiceExit {
    /// Check if this represents a successful exit (code 0)
    pub fn is_success(&self) -> bool {
        self.exit_code == Some(0)
    }
    
    /// Check if this represents a failure (non-zero exit code or signal)
    pub fn is_failure(&self) -> bool {
        !self.is_success()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_service_spec_defaults() {
        let spec = ServiceSpec {
            id: "test-service".to_string(),
            name: "Test Service".to_string(),
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            environment: HashMap::new(),
            working_directory: None,
            restart_policy: RestartPolicy::default(),
            backoff_config: BackoffConfig::default(),
            health_check: None,
            readiness_check: None,
            graceful_timeout_secs: default_graceful_timeout_secs(),
            startup_timeout_secs: default_startup_timeout_secs(),
        };
        
        assert_eq!(spec.graceful_timeout(), Duration::from_secs(30));
        assert_eq!(spec.startup_timeout(), Duration::from_secs(60));
    }

    #[test]
    fn test_service_state_predicates() {
        assert!(!ServiceState::Idle.is_running());
        assert!(!ServiceState::Idle.is_ready());
        assert!(!ServiceState::Idle.is_transitional());

        assert!(ServiceState::Spawning.is_running());
        assert!(!ServiceState::Spawning.is_ready());
        assert!(ServiceState::Spawning.is_transitional());

        assert!(ServiceState::Ready.is_running());
        assert!(ServiceState::Ready.is_ready());
        assert!(!ServiceState::Ready.is_transitional());

        assert!(ServiceState::Stopping.is_running());
        assert!(!ServiceState::Stopping.is_ready());
        assert!(ServiceState::Stopping.is_transitional());
    }

    #[test]
    fn test_restart_policy_default() {
        assert_eq!(RestartPolicy::default(), RestartPolicy::Always);
    }

    #[test]
    fn test_backoff_config_durations() {
        let config = BackoffConfig::default();
        assert_eq!(config.base_delay(), Duration::from_secs(1));
        assert_eq!(config.max_delay(), Duration::from_secs(300));
        assert_eq!(config.failure_window(), Duration::from_secs(900));
    }

    #[test]
    fn test_health_check_durations() {
        let health = HealthCheck {
            check_type: HealthCheckType::Tcp { port: 8080 },
            interval_secs: 10,
            timeout_secs: 5,
            failure_threshold: 3,
            success_threshold: 1,
        };
        
        assert_eq!(health.interval(), Duration::from_secs(10));
        assert_eq!(health.timeout(), Duration::from_secs(5));
    }

    #[test]
    fn test_readiness_check_durations() {
        let readiness = ReadinessCheck {
            check_type: HealthCheckType::Http {
                port: 8080,
                path: "/ready".to_string(),
                success_codes: vec![200, 204],
            },
            initial_delay_secs: 5,
            interval_secs: 2,
            timeout_secs: 3,
            success_threshold: 1,
        };
        
        assert_eq!(readiness.initial_delay(), Duration::from_secs(5));
        assert_eq!(readiness.interval(), Duration::from_secs(2));
        assert_eq!(readiness.timeout(), Duration::from_secs(3));
    }

    #[test]
    fn test_service_exit_predicates() {
        let success_exit = ServiceExit {
            pid: 1234,
            exit_code: Some(0),
            signal: None,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        assert!(success_exit.is_success());
        assert!(!success_exit.is_failure());

        let failure_exit = ServiceExit {
            pid: 1235,
            exit_code: Some(1),
            signal: None,
            timestamp: "2024-01-01T00:00:01Z".to_string(),
        };
        assert!(!failure_exit.is_success());
        assert!(failure_exit.is_failure());

        let signal_exit = ServiceExit {
            pid: 1236,
            exit_code: None,
            signal: Some(9),
            timestamp: "2024-01-01T00:00:02Z".to_string(),
        };
        assert!(!signal_exit.is_success());
        assert!(signal_exit.is_failure());
    }

    #[test]
    fn test_health_check_type_tcp() {
        let tcp_check = HealthCheckType::Tcp { port: 8080 };
        match tcp_check {
            HealthCheckType::Tcp { port } => assert_eq!(port, 8080),
            _ => panic!("Expected TCP check"),
        }
    }

    #[test]
    fn test_health_check_type_http_defaults() {
        let http_check = HealthCheckType::Http {
            port: 8080,
            path: default_health_path(),
            success_codes: default_success_codes(),
        };
        
        match http_check {
            HealthCheckType::Http { port, path, success_codes } => {
                assert_eq!(port, 8080);
                assert_eq!(path, "/health");
                assert_eq!(success_codes, vec![200]);
            }
            _ => panic!("Expected HTTP check"),
        }
    }

    #[test]
    fn test_health_check_type_exec() {
        let exec_check = HealthCheckType::Exec {
            command: "curl".to_string(),
            args: vec!["-f", "http://localhost:8080/health"].iter().map(|s| s.to_string()).collect(),
        };
        
        match exec_check {
            HealthCheckType::Exec { command, args } => {
                assert_eq!(command, "curl");
                assert_eq!(args.len(), 2);
            }
            _ => panic!("Expected Exec check"),
        }
    }
}
