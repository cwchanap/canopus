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
            Self::RestartScheduled { .. }
            | Self::Warning { .. }
            | Self::StartupTimeout { .. } => EventSeverity::Warning,
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
}
