//! Event system for the Canopus supervisor
//!
//! This module defines the event types that can be emitted by the supervisor
//! system to provide observability into service state changes, process lifecycle
//! events, and health check results.
//!
//! Events are designed to be serializable and can be:
//! - Logged to structured log files
//! - Sent to monitoring systems
//! - Used for debugging and operational visibility
//! - Broadcast to multiple subscribers via event channels

use crate::service::{ServiceExit, ServiceState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Events emitted by the supervisor system
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "eventType", rename_all = "camelCase")]
pub enum ServiceEvent {
    /// Service state has changed
    StateChanged {
        /// Service identifier
        service_id: String,
        /// Previous state
        from_state: ServiceState,
        /// New state
        to_state: ServiceState,
        /// Event timestamp in RFC3339 format
        timestamp: String,
        /// Optional reason for the state change
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// Service process has started
    ProcessStarted {
        /// Service identifier
        service_id: String,
        /// Process ID of the started service
        pid: u32,
        /// Event timestamp in RFC3339 format
        timestamp: String,
        /// Command that was executed
        command: String,
        /// Arguments passed to the command
        args: Vec<String>,
    },

    /// Service process has exited
    ProcessExited {
        /// Service identifier
        service_id: String,
        /// Exit information
        exit_info: ServiceExit,
    },

    /// Health check result
    HealthCheckResult {
        /// Service identifier
        service_id: String,
        /// Whether the health check passed
        success: bool,
        /// Event timestamp in RFC3339 format
        timestamp: String,
        /// Optional error message if the check failed
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// Duration of the health check in milliseconds
        duration_ms: u64,
    },

    /// Readiness check result
    ReadinessCheckResult {
        /// Service identifier
        service_id: String,
        /// Whether the readiness check passed
        success: bool,
        /// Event timestamp in RFC3339 format
        timestamp: String,
        /// Optional error message if the check failed
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// Duration of the readiness check in milliseconds
        duration_ms: u64,
    },

    /// Service restart is scheduled
    RestartScheduled {
        /// Service identifier
        service_id: String,
        /// Delay before restart in seconds
        delay_secs: u64,
        /// Number of restart attempts so far
        attempt_count: u32,
        /// Event timestamp in RFC3339 format
        timestamp: String,
        /// Reason for the restart
        reason: String,
    },

    /// Service restart attempt has begun
    RestartAttempt {
        /// Service identifier
        service_id: String,
        /// Restart attempt number (1-indexed)
        attempt: u32,
        /// Event timestamp in RFC3339 format
        timestamp: String,
    },

    /// Service configuration has been updated
    ConfigurationUpdated {
        /// Service identifier
        service_id: String,
        /// Event timestamp in RFC3339 format
        timestamp: String,
        /// Fields that were changed
        changed_fields: Vec<String>,
    },

    /// A route has been attached to the service
    RouteAttached {
        /// Service identifier
        service_id: String,
        /// Route that was attached (e.g., "/api/v1")
        route: String,
        /// Normalized host component if host-based routing was used
        #[serde(skip_serializing_if = "Option::is_none")]
        route_host: Option<String>,
        /// Normalized path component if path-based routing was used
        #[serde(skip_serializing_if = "Option::is_none")]
        route_path: Option<String>,
        /// Backend address where requests are routed
        backend_address: String,
        /// Event timestamp in RFC3339 format
        timestamp: String,
    },

    /// A route has been detached from the service
    RouteDetached {
        /// Service identifier
        service_id: String,
        /// Route that was detached
        route: String,
        /// Normalized host component if host-based routing was used
        #[serde(skip_serializing_if = "Option::is_none")]
        route_host: Option<String>,
        /// Normalized path component if path-based routing was used
        #[serde(skip_serializing_if = "Option::is_none")]
        route_path: Option<String>,
        /// Event timestamp in RFC3339 format
        timestamp: String,
    },

    /// Service logs have been captured
    LogOutput {
        /// Service identifier
        service_id: String,
        /// Log stream (stdout or stderr)
        stream: LogStream,
        /// Log content
        content: String,
        /// Event timestamp in RFC3339 format
        timestamp: String,
    },

    /// A warning condition has occurred
    Warning {
        /// Service identifier
        service_id: String,
        /// Warning message
        message: String,
        /// Event timestamp in RFC3339 format
        timestamp: String,
        /// Optional warning code for categorization
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },

    /// An error condition has occurred
    Error {
        /// Service identifier
        service_id: String,
        /// Error message
        message: String,
        /// Event timestamp in RFC3339 format
        timestamp: String,
        /// Optional error code for categorization
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },

    /// Startup timeout has expired
    StartupTimeout {
        /// Service identifier
        service_id: String,
        /// Event timestamp in RFC3339 format
        timestamp: String,
        /// Configured startup timeout in seconds
        timeout_secs: u64,
    },

    /// Service has become unhealthy due to failed health checks
    ServiceUnhealthy {
        /// Service identifier
        service_id: String,
        /// Event timestamp in RFC3339 format
        timestamp: String,
        /// Reason for being unhealthy
        reason: String,
        /// Number of consecutive failures that triggered this event
        consecutive_failures: u32,
    },
}

/// Log stream identifier
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LogStream {
    /// Standard output
    Stdout,
    /// Standard error
    Stderr,
}

/// Event severity level for filtering and alerting
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "camelCase")]
pub enum EventSeverity {
    /// Debug information
    Debug,
    /// Informational events
    Info,
    /// Warning conditions
    Warning,
    /// Error conditions
    Error,
    /// Critical conditions requiring immediate attention
    Critical,
}

impl ServiceEvent {
    /// Get the service ID for this event
    #[must_use]
    pub fn service_id(&self) -> &str {
        match self {
            Self::StateChanged { service_id, .. }
            | Self::ProcessStarted { service_id, .. }
            | Self::ProcessExited { service_id, .. }
            | Self::HealthCheckResult { service_id, .. }
            | Self::ReadinessCheckResult { service_id, .. }
            | Self::RestartScheduled { service_id, .. }
            | Self::RestartAttempt { service_id, .. }
            | Self::ConfigurationUpdated { service_id, .. }
            | Self::RouteAttached { service_id, .. }
            | Self::RouteDetached { service_id, .. }
            | Self::LogOutput { service_id, .. }
            | Self::Warning { service_id, .. }
            | Self::Error { service_id, .. }
            | Self::StartupTimeout { service_id, .. }
            | Self::ServiceUnhealthy { service_id, .. } => service_id,
        }
    }

    /// Get the timestamp for this event
    #[must_use]
    pub fn timestamp(&self) -> &str {
        match self {
            Self::ProcessExited { exit_info, .. } => &exit_info.timestamp,
            Self::StateChanged { timestamp, .. }
            | Self::ProcessStarted { timestamp, .. }
            | Self::HealthCheckResult { timestamp, .. }
            | Self::ReadinessCheckResult { timestamp, .. }
            | Self::RestartScheduled { timestamp, .. }
            | Self::RestartAttempt { timestamp, .. }
            | Self::ConfigurationUpdated { timestamp, .. }
            | Self::RouteAttached { timestamp, .. }
            | Self::RouteDetached { timestamp, .. }
            | Self::LogOutput { timestamp, .. }
            | Self::Warning { timestamp, .. }
            | Self::Error { timestamp, .. }
            | Self::StartupTimeout { timestamp, .. }
            | Self::ServiceUnhealthy { timestamp, .. } => timestamp,
        }
    }

    /// Get the severity level for this event
    #[must_use]
    pub fn severity(&self) -> EventSeverity {
        match self {
            Self::StateChanged { .. }
            | Self::ProcessStarted { .. }
            | Self::RestartAttempt { .. }
            | Self::ConfigurationUpdated { .. }
            | Self::RouteAttached { .. }
            | Self::RouteDetached { .. } => EventSeverity::Info,
            Self::ProcessExited { exit_info, .. } => {
                if exit_info.is_success() {
                    EventSeverity::Info
                } else {
                    EventSeverity::Warning
                }
            }
            Self::HealthCheckResult { success, .. }
            | Self::ReadinessCheckResult { success, .. } => {
                if *success {
                    EventSeverity::Debug
                } else {
                    EventSeverity::Warning
                }
            }
            Self::RestartScheduled { .. } | Self::Warning { .. } | Self::StartupTimeout { .. } => {
                EventSeverity::Warning
            }
            Self::LogOutput { .. } => EventSeverity::Debug,
            Self::Error { .. } => EventSeverity::Error,
            Self::ServiceUnhealthy { .. } => EventSeverity::Critical,
        }
    }

    /// Create a current timestamp string in RFC3339 format
    #[must_use]
    pub fn current_timestamp() -> String {
        // Simple RFC3339 format: YYYY-MM-DDTHH:MM:SSZ
        // For production use, consider using chrono for proper timezone handling
        format!(
            "{}Z",
            humantime::format_rfc3339_seconds(SystemTime::now())
                .to_string()
                .trim_end_matches(".000000000Z")
        )
    }

    /// Create a state changed event
    #[must_use]
    pub fn state_changed(
        service_id: String,
        from_state: ServiceState,
        to_state: ServiceState,
        reason: Option<String>,
    ) -> Self {
        Self::StateChanged {
            service_id,
            from_state,
            to_state,
            timestamp: Self::current_timestamp(),
            reason,
        }
    }

    /// Create a process started event
    #[must_use]
    pub fn process_started(
        service_id: String,
        pid: u32,
        command: String,
        args: Vec<String>,
    ) -> Self {
        Self::ProcessStarted {
            service_id,
            pid,
            timestamp: Self::current_timestamp(),
            command,
            args,
        }
    }

    /// Create a process exited event
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn process_exited(service_id: String, exit_info: ServiceExit) -> Self {
        Self::ProcessExited {
            service_id,
            exit_info,
        }
    }

    /// Create a health check result event
    #[must_use]
    pub fn health_check_result(
        service_id: String,
        success: bool,
        error: Option<String>,
        duration_ms: u64,
    ) -> Self {
        Self::HealthCheckResult {
            service_id,
            success,
            timestamp: Self::current_timestamp(),
            error,
            duration_ms,
        }
    }

    /// Create a readiness check result event
    #[must_use]
    pub fn readiness_check_result(
        service_id: String,
        success: bool,
        error: Option<String>,
        duration_ms: u64,
    ) -> Self {
        Self::ReadinessCheckResult {
            service_id,
            success,
            timestamp: Self::current_timestamp(),
            error,
            duration_ms,
        }
    }

    /// Create a restart scheduled event
    #[must_use]
    pub fn restart_scheduled(
        service_id: String,
        delay_secs: u64,
        attempt_count: u32,
        reason: String,
    ) -> Self {
        Self::RestartScheduled {
            service_id,
            delay_secs,
            attempt_count,
            timestamp: Self::current_timestamp(),
            reason,
        }
    }

    /// Create a restart attempt event
    #[must_use]
    pub fn restart_attempt(service_id: String, attempt: u32) -> Self {
        Self::RestartAttempt {
            service_id,
            attempt,
            timestamp: Self::current_timestamp(),
        }
    }

    /// Create a warning event
    #[must_use]
    pub fn warning(service_id: String, message: String, code: Option<String>) -> Self {
        Self::Warning {
            service_id,
            message,
            timestamp: Self::current_timestamp(),
            code,
        }
    }

    /// Create an error event
    #[must_use]
    pub fn error(service_id: String, message: String, code: Option<String>) -> Self {
        Self::Error {
            service_id,
            message,
            timestamp: Self::current_timestamp(),
            code,
        }
    }
}

/// Event filter for subscribing to specific events
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventFilter {
    /// Filter by service IDs (empty means all services)
    #[serde(default)]
    pub service_ids: Vec<String>,

    /// Filter by minimum severity level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_severity: Option<EventSeverity>,

    /// Filter by event types (empty means all types)
    #[serde(default)]
    pub event_types: Vec<String>,
}

impl EventFilter {
    /// Create a filter that matches all events
    #[must_use]
    pub const fn all() -> Self {
        Self {
            service_ids: Vec::new(),
            min_severity: None,
            event_types: Vec::new(),
        }
    }

    /// Create a filter for a specific service
    #[must_use]
    pub fn for_service(service_id: String) -> Self {
        Self {
            service_ids: vec![service_id],
            min_severity: None,
            event_types: Vec::new(),
        }
    }

    /// Create a filter for events with minimum severity
    #[must_use]
    pub const fn with_min_severity(severity: EventSeverity) -> Self {
        Self {
            service_ids: Vec::new(),
            min_severity: Some(severity),
            event_types: Vec::new(),
        }
    }

    /// Check if this filter matches the given event
    #[must_use]
    pub fn matches(&self, event: &ServiceEvent) -> bool {
        // Check service ID filter
        if !self.service_ids.is_empty()
            && !self.service_ids.contains(&event.service_id().to_string())
        {
            return false;
        }

        // Check severity filter
        if let Some(min_severity) = self.min_severity {
            if event.severity() < min_severity {
                return false;
            }
        }

        // Check event type filter
        if !self.event_types.is_empty() {
            let event_type = match event {
                ServiceEvent::StateChanged { .. } => "stateChanged",
                ServiceEvent::ProcessStarted { .. } => "processStarted",
                ServiceEvent::ProcessExited { .. } => "processExited",
                ServiceEvent::HealthCheckResult { .. } => "healthCheckResult",
                ServiceEvent::ReadinessCheckResult { .. } => "readinessCheckResult",
                ServiceEvent::RestartScheduled { .. } => "restartScheduled",
                ServiceEvent::RestartAttempt { .. } => "restartAttempt",
                ServiceEvent::ConfigurationUpdated { .. } => "configurationUpdated",
                ServiceEvent::RouteAttached { .. } => "routeAttached",
                ServiceEvent::RouteDetached { .. } => "routeDetached",
                ServiceEvent::LogOutput { .. } => "logOutput",
                ServiceEvent::Warning { .. } => "warning",
                ServiceEvent::Error { .. } => "error",
                ServiceEvent::StartupTimeout { .. } => "startupTimeout",
                ServiceEvent::ServiceUnhealthy { .. } => "serviceUnhealthy",
            };

            if !self.event_types.contains(&event_type.to_string()) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::ServiceState;

    #[test]
    fn test_service_event_service_id() {
        let event = ServiceEvent::state_changed(
            "test-service".to_string(),
            ServiceState::Idle,
            ServiceState::Spawning,
            None,
        );

        assert_eq!(event.service_id(), "test-service");
    }

    #[test]
    fn test_service_event_severity() {
        // Info severity
        let state_change = ServiceEvent::state_changed(
            "test".to_string(),
            ServiceState::Idle,
            ServiceState::Ready,
            None,
        );
        assert_eq!(state_change.severity(), EventSeverity::Info);

        // Warning severity for failed health check
        let health_check = ServiceEvent::health_check_result(
            "test".to_string(),
            false,
            Some("Connection refused".to_string()),
            100,
        );
        assert_eq!(health_check.severity(), EventSeverity::Warning);

        // Error severity
        let error = ServiceEvent::error(
            "test".to_string(),
            "Failed to start".to_string(),
            Some("CORE001".to_string()),
        );
        assert_eq!(error.severity(), EventSeverity::Error);
    }

    #[test]
    fn test_service_event_constructors() {
        let service_id = "test-service".to_string();

        // Test state changed event
        let state_event = ServiceEvent::state_changed(
            service_id.clone(),
            ServiceState::Idle,
            ServiceState::Ready,
            Some("Health check passed".to_string()),
        );

        match state_event {
            ServiceEvent::StateChanged {
                from_state,
                to_state,
                reason,
                ..
            } => {
                assert_eq!(from_state, ServiceState::Idle);
                assert_eq!(to_state, ServiceState::Ready);
                assert_eq!(reason, Some("Health check passed".to_string()));
            }
            _ => panic!("Expected StateChanged event"),
        }

        // Test process started event
        let process_event = ServiceEvent::process_started(
            service_id.clone(),
            1234,
            "echo".to_string(),
            vec!["hello".to_string()],
        );

        match process_event {
            ServiceEvent::ProcessStarted {
                pid, command, args, ..
            } => {
                assert_eq!(pid, 1234);
                assert_eq!(command, "echo");
                assert_eq!(args, vec!["hello"]);
            }
            _ => panic!("Expected ProcessStarted event"),
        }

        // Test warning event
        let warning_event = ServiceEvent::warning(
            service_id,
            "Service is slow to respond".to_string(),
            Some("WARN001".to_string()),
        );

        match warning_event {
            ServiceEvent::Warning { message, code, .. } => {
                assert_eq!(message, "Service is slow to respond");
                assert_eq!(code, Some("WARN001".to_string()));
            }
            _ => panic!("Expected Warning event"),
        }
    }

    #[test]
    fn test_event_filter_all() {
        let filter = EventFilter::all();
        let event = ServiceEvent::state_changed(
            "test".to_string(),
            ServiceState::Idle,
            ServiceState::Ready,
            None,
        );

        assert!(filter.matches(&event));
    }

    #[test]
    fn test_event_filter_service() {
        let filter = EventFilter::for_service("test-service".to_string());

        let matching_event = ServiceEvent::state_changed(
            "test-service".to_string(),
            ServiceState::Idle,
            ServiceState::Ready,
            None,
        );

        let non_matching_event = ServiceEvent::state_changed(
            "other-service".to_string(),
            ServiceState::Idle,
            ServiceState::Ready,
            None,
        );

        assert!(filter.matches(&matching_event));
        assert!(!filter.matches(&non_matching_event));
    }

    #[test]
    fn test_event_filter_severity() {
        let filter = EventFilter::with_min_severity(EventSeverity::Warning);

        let warning_event =
            ServiceEvent::warning("test".to_string(), "Warning message".to_string(), None);

        let info_event = ServiceEvent::state_changed(
            "test".to_string(),
            ServiceState::Idle,
            ServiceState::Ready,
            None,
        );

        assert!(filter.matches(&warning_event));
        assert!(!filter.matches(&info_event));
    }

    #[test]
    fn test_event_filter_type() {
        let filter = EventFilter {
            service_ids: Vec::new(),
            min_severity: None,
            event_types: vec!["stateChanged".to_string()],
        };

        let state_event = ServiceEvent::state_changed(
            "test".to_string(),
            ServiceState::Idle,
            ServiceState::Ready,
            None,
        );

        let warning_event = ServiceEvent::warning("test".to_string(), "Warning".to_string(), None);

        assert!(filter.matches(&state_event));
        assert!(!filter.matches(&warning_event));
    }

    #[test]
    fn test_current_timestamp_format() {
        let timestamp = ServiceEvent::current_timestamp();

        // Should be roughly RFC3339 format
        // Just check it's not empty and contains expected parts
        assert!(!timestamp.is_empty());
        assert!(timestamp.contains('T'));
        assert!(timestamp.ends_with('Z'));
    }

    #[test]
    fn test_log_stream_serialization() {
        let stdout = LogStream::Stdout;
        let stderr = LogStream::Stderr;

        // Should serialize to camelCase
        let stdout_json = serde_json::to_string(&stdout).unwrap();
        let stderr_json = serde_json::to_string(&stderr).unwrap();

        assert_eq!(stdout_json, "\"stdout\"");
        assert_eq!(stderr_json, "\"stderr\"");
    }

    #[test]
    fn test_event_severity_ordering() {
        assert!(EventSeverity::Debug < EventSeverity::Info);
        assert!(EventSeverity::Info < EventSeverity::Warning);
        assert!(EventSeverity::Warning < EventSeverity::Error);
        assert!(EventSeverity::Error < EventSeverity::Critical);
    }

    #[test]
    fn test_service_event_timestamp_for_all_variants() {
        let ts = "2024-06-01T12:00:00Z".to_string();
        let exit_info = crate::service::ServiceExit {
            pid: 1,
            exit_code: Some(0),
            signal: None,
            timestamp: ts.clone(),
        };

        // ProcessExited gets timestamp from exit_info.timestamp
        let process_exited = ServiceEvent::ProcessExited {
            service_id: "s".to_string(),
            exit_info: exit_info.clone(),
        };
        assert_eq!(process_exited.timestamp(), ts);

        // All other variants store their own timestamp field
        let startup_timeout = ServiceEvent::StartupTimeout {
            service_id: "s".to_string(),
            timestamp: ts.clone(),
            timeout_secs: 60,
        };
        assert_eq!(startup_timeout.timestamp(), ts);

        let unhealthy = ServiceEvent::ServiceUnhealthy {
            service_id: "s".to_string(),
            timestamp: ts.clone(),
            reason: "checks failed".to_string(),
            consecutive_failures: 3,
        };
        assert_eq!(unhealthy.timestamp(), ts);

        let log_output = ServiceEvent::LogOutput {
            service_id: "s".to_string(),
            stream: LogStream::Stdout,
            content: "hello".to_string(),
            timestamp: ts.clone(),
        };
        assert_eq!(log_output.timestamp(), ts);

        let route_attached = ServiceEvent::RouteAttached {
            service_id: "s".to_string(),
            route: "/".to_string(),
            route_host: None,
            route_path: None,
            backend_address: "127.0.0.1:8080".to_string(),
            timestamp: ts.clone(),
        };
        assert_eq!(route_attached.timestamp(), ts);

        let route_detached = ServiceEvent::RouteDetached {
            service_id: "s".to_string(),
            route: "/".to_string(),
            route_host: None,
            route_path: None,
            timestamp: ts.clone(),
        };
        assert_eq!(route_detached.timestamp(), ts);
    }

    #[test]
    fn test_startup_timeout_and_service_unhealthy_severity() {
        let startup_timeout = ServiceEvent::StartupTimeout {
            service_id: "s".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            timeout_secs: 60,
        };
        assert_eq!(startup_timeout.severity(), EventSeverity::Warning);

        let unhealthy = ServiceEvent::ServiceUnhealthy {
            service_id: "s".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            reason: "3 consecutive failures".to_string(),
            consecutive_failures: 3,
        };
        assert_eq!(unhealthy.severity(), EventSeverity::Critical);
    }

    #[test]
    fn test_log_output_and_configuration_updated_severity() {
        let log_output = ServiceEvent::LogOutput {
            service_id: "s".to_string(),
            stream: LogStream::Stderr,
            content: "some log".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        assert_eq!(log_output.severity(), EventSeverity::Debug);

        let config_updated = ServiceEvent::ConfigurationUpdated {
            service_id: "s".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            changed_fields: vec!["command".to_string()],
        };
        assert_eq!(config_updated.severity(), EventSeverity::Info);
    }

    #[test]
    fn test_route_attached_detached_severity() {
        let route_attached = ServiceEvent::RouteAttached {
            service_id: "s".to_string(),
            route: "/api".to_string(),
            route_host: Some("api.dev".to_string()),
            route_path: Some("/api".to_string()),
            backend_address: "127.0.0.1:9000".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        assert_eq!(route_attached.severity(), EventSeverity::Info);
        assert_eq!(route_attached.service_id(), "s");

        let route_detached = ServiceEvent::RouteDetached {
            service_id: "s".to_string(),
            route: "/api".to_string(),
            route_host: None,
            route_path: None,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        assert_eq!(route_detached.severity(), EventSeverity::Info);
        assert_eq!(route_detached.service_id(), "s");
    }

    #[test]
    fn test_process_exited_severity_success_vs_failure() {
        let success_exit = crate::service::ServiceExit {
            pid: 1,
            exit_code: Some(0),
            signal: None,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let event_success = ServiceEvent::ProcessExited {
            service_id: "s".to_string(),
            exit_info: success_exit,
        };
        assert_eq!(event_success.severity(), EventSeverity::Info);

        let failure_exit = crate::service::ServiceExit {
            pid: 2,
            exit_code: Some(1),
            signal: None,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let event_failure = ServiceEvent::ProcessExited {
            service_id: "s".to_string(),
            exit_info: failure_exit,
        };
        assert_eq!(event_failure.severity(), EventSeverity::Warning);
    }

    #[test]
    fn test_readiness_check_result_severity() {
        let success = ServiceEvent::ReadinessCheckResult {
            service_id: "s".to_string(),
            success: true,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            error: None,
            duration_ms: 10,
        };
        assert_eq!(success.severity(), EventSeverity::Debug);

        let failure = ServiceEvent::ReadinessCheckResult {
            service_id: "s".to_string(),
            success: false,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            error: Some("timeout".to_string()),
            duration_ms: 3000,
        };
        assert_eq!(failure.severity(), EventSeverity::Warning);
    }

    #[test]
    fn test_event_filter_combined_service_and_severity() {
        let filter = EventFilter {
            service_ids: vec!["svc-a".to_string()],
            min_severity: Some(EventSeverity::Warning),
            event_types: Vec::new(),
        };

        // Matches: right service, high enough severity
        let warn_a = ServiceEvent::Warning {
            service_id: "svc-a".to_string(),
            message: "watch out".to_string(),
            timestamp: "t".to_string(),
            code: None,
        };
        assert!(filter.matches(&warn_a));

        // No match: wrong service, even with high severity
        let warn_b = ServiceEvent::Warning {
            service_id: "svc-b".to_string(),
            message: "watch out".to_string(),
            timestamp: "t".to_string(),
            code: None,
        };
        assert!(!filter.matches(&warn_b));

        // No match: right service, severity too low (Info < Warning)
        let info_a = ServiceEvent::StateChanged {
            service_id: "svc-a".to_string(),
            from_state: ServiceState::Idle,
            to_state: ServiceState::Ready,
            timestamp: "t".to_string(),
            reason: None,
        };
        assert!(!filter.matches(&info_a));
    }

    #[test]
    fn test_event_filter_by_type_multiple() {
        let filter = EventFilter {
            service_ids: Vec::new(),
            min_severity: None,
            event_types: vec!["processStarted".to_string(), "processExited".to_string()],
        };

        let started = ServiceEvent::ProcessStarted {
            service_id: "s".to_string(),
            pid: 1,
            timestamp: "t".to_string(),
            command: "echo".to_string(),
            args: vec![],
        };
        assert!(filter.matches(&started));

        let exited = ServiceEvent::ProcessExited {
            service_id: "s".to_string(),
            exit_info: crate::service::ServiceExit {
                pid: 1,
                exit_code: Some(0),
                signal: None,
                timestamp: "t".to_string(),
            },
        };
        assert!(filter.matches(&exited));

        // Does not match stateChanged
        let state = ServiceEvent::StateChanged {
            service_id: "s".to_string(),
            from_state: ServiceState::Idle,
            to_state: ServiceState::Ready,
            timestamp: "t".to_string(),
            reason: None,
        };
        assert!(!filter.matches(&state));
    }

    #[test]
    fn test_event_constructor_process_exited() {
        let exit_info = crate::service::ServiceExit {
            pid: 42,
            exit_code: Some(0),
            signal: None,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let event = ServiceEvent::process_exited("my-svc".to_string(), exit_info.clone());
        assert_eq!(event.service_id(), "my-svc");
        match event {
            ServiceEvent::ProcessExited {
                service_id,
                exit_info: ei,
            } => {
                assert_eq!(service_id, "my-svc");
                assert_eq!(ei.pid, 42);
            }
            _ => panic!("Expected ProcessExited"),
        }
    }

    #[test]
    fn test_event_constructor_readiness_check_result() {
        let event = ServiceEvent::readiness_check_result("svc".to_string(), true, None, 25);
        assert_eq!(event.service_id(), "svc");
        match event {
            ServiceEvent::ReadinessCheckResult {
                success,
                duration_ms,
                ..
            } => {
                assert!(success);
                assert_eq!(duration_ms, 25);
            }
            _ => panic!("Expected ReadinessCheckResult"),
        }
    }

    #[test]
    fn test_event_constructor_restart_scheduled_and_attempt() {
        let scheduled =
            ServiceEvent::restart_scheduled("svc".to_string(), 5, 2, "crashed".to_string());
        assert_eq!(scheduled.service_id(), "svc");
        assert_eq!(scheduled.severity(), EventSeverity::Warning);

        let attempt = ServiceEvent::restart_attempt("svc".to_string(), 3);
        assert_eq!(attempt.service_id(), "svc");
        assert_eq!(attempt.severity(), EventSeverity::Info);
        match attempt {
            ServiceEvent::RestartAttempt { attempt, .. } => assert_eq!(attempt, 3),
            _ => panic!("Expected RestartAttempt"),
        }
    }

    #[test]
    fn test_event_filter_type_all_known_event_types() {
        // Verify every event type string is correctly matched by the filter
        let all_types = vec![
            "stateChanged",
            "processStarted",
            "processExited",
            "healthCheckResult",
            "readinessCheckResult",
            "restartScheduled",
            "restartAttempt",
            "configurationUpdated",
            "routeAttached",
            "routeDetached",
            "logOutput",
            "warning",
            "error",
            "startupTimeout",
            "serviceUnhealthy",
        ];
        let exit_info = crate::service::ServiceExit {
            pid: 1,
            exit_code: Some(0),
            signal: None,
            timestamp: "t".to_string(),
        };
        let events: Vec<ServiceEvent> = vec![
            ServiceEvent::StateChanged {
                service_id: "s".to_string(),
                from_state: ServiceState::Idle,
                to_state: ServiceState::Ready,
                timestamp: "t".to_string(),
                reason: None,
            },
            ServiceEvent::ProcessStarted {
                service_id: "s".to_string(),
                pid: 1,
                timestamp: "t".to_string(),
                command: "echo".to_string(),
                args: vec![],
            },
            ServiceEvent::ProcessExited {
                service_id: "s".to_string(),
                exit_info: exit_info.clone(),
            },
            ServiceEvent::HealthCheckResult {
                service_id: "s".to_string(),
                success: true,
                timestamp: "t".to_string(),
                error: None,
                duration_ms: 1,
            },
            ServiceEvent::ReadinessCheckResult {
                service_id: "s".to_string(),
                success: true,
                timestamp: "t".to_string(),
                error: None,
                duration_ms: 1,
            },
            ServiceEvent::RestartScheduled {
                service_id: "s".to_string(),
                delay_secs: 1,
                attempt_count: 1,
                timestamp: "t".to_string(),
                reason: "r".to_string(),
            },
            ServiceEvent::RestartAttempt {
                service_id: "s".to_string(),
                attempt: 1,
                timestamp: "t".to_string(),
            },
            ServiceEvent::ConfigurationUpdated {
                service_id: "s".to_string(),
                timestamp: "t".to_string(),
                changed_fields: vec![],
            },
            ServiceEvent::RouteAttached {
                service_id: "s".to_string(),
                route: "/".to_string(),
                route_host: None,
                route_path: None,
                backend_address: "127.0.0.1:1".to_string(),
                timestamp: "t".to_string(),
            },
            ServiceEvent::RouteDetached {
                service_id: "s".to_string(),
                route: "/".to_string(),
                route_host: None,
                route_path: None,
                timestamp: "t".to_string(),
            },
            ServiceEvent::LogOutput {
                service_id: "s".to_string(),
                stream: LogStream::Stdout,
                content: "x".to_string(),
                timestamp: "t".to_string(),
            },
            ServiceEvent::Warning {
                service_id: "s".to_string(),
                message: "w".to_string(),
                timestamp: "t".to_string(),
                code: None,
            },
            ServiceEvent::Error {
                service_id: "s".to_string(),
                message: "e".to_string(),
                timestamp: "t".to_string(),
                code: None,
            },
            ServiceEvent::StartupTimeout {
                service_id: "s".to_string(),
                timestamp: "t".to_string(),
                timeout_secs: 60,
            },
            ServiceEvent::ServiceUnhealthy {
                service_id: "s".to_string(),
                timestamp: "t".to_string(),
                reason: "r".to_string(),
                consecutive_failures: 1,
            },
        ];
        assert_eq!(all_types.len(), events.len());
        for (type_str, event) in all_types.iter().zip(events.iter()) {
            let filter = EventFilter {
                service_ids: Vec::new(),
                min_severity: None,
                event_types: vec![type_str.to_string()],
            };
            assert!(
                filter.matches(event),
                "Filter for type '{type_str}' should match its event"
            );
        }
    }
}
