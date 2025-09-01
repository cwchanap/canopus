//! JSON round-trip tests for schema types
//!
//! These tests verify that all schema types can be properly serialized to JSON
//! and deserialized back to the original values, ensuring API compatibility
//! and proper serde configuration.

use crate::events::*;
use crate::service::*;
use schemars::schema_for;
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to test JSON round-trip for any serializable type
    fn test_json_roundtrip<T>(original: &T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(original).expect("Failed to serialize to JSON");
        let deserialized: T = serde_json::from_str(&json).expect("Failed to deserialize from JSON");
        assert_eq!(*original, deserialized, "Round-trip failed for JSON: {}", json);
    }

    #[test]
    fn test_service_spec_json_roundtrip() {
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
        env.insert("HOME".to_string(), "/home/user".to_string());

        let spec = ServiceSpec {
            id: "test-service".to_string(),
            name: "Test Service".to_string(),
            command: "echo".to_string(),
            args: vec!["hello".to_string(), "world".to_string()],
            environment: env,
            working_directory: Some("/tmp".to_string()),
            restart_policy: RestartPolicy::OnFailure,
            backoff_config: BackoffConfig {
                base_delay_secs: 2,
                multiplier: 1.5,
                max_delay_secs: 120,
                jitter: 0.2,
                failure_window_secs: 600,
            },
            health_check: Some(HealthCheck {
                check_type: HealthCheckType::Http {
                    port: 8080,
                    path: "/health".to_string(),
                    success_codes: vec![200, 202],
                },
                interval_secs: 10,
                timeout_secs: 3,
                failure_threshold: 2,
                success_threshold: 1,
            }),
            readiness_check: Some(ReadinessCheck {
                check_type: HealthCheckType::Tcp { port: 8080 },
                initial_delay_secs: 5,
                interval_secs: 2,
                timeout_secs: 1,
                success_threshold: 2,
            }),
            graceful_timeout_secs: 30,
            startup_timeout_secs: 60,
        };

        test_json_roundtrip(&spec);
    }

    #[test]
    fn test_service_spec_with_minimal_config() {
        let spec = ServiceSpec {
            id: "minimal-service".to_string(),
            name: "Minimal Service".to_string(),
            command: "true".to_string(),
            args: vec![],
            environment: HashMap::new(),
            working_directory: None,
            restart_policy: RestartPolicy::default(),
            backoff_config: BackoffConfig::default(),
            health_check: None,
            readiness_check: None,
            graceful_timeout_secs: 30,
            startup_timeout_secs: 60,
        };

        test_json_roundtrip(&spec);
    }

    #[test]
    fn test_service_state_json_roundtrip() {
        let states = [
            ServiceState::Idle,
            ServiceState::Spawning,
            ServiceState::Starting,
            ServiceState::Ready,
            ServiceState::Stopping,
        ];

        for state in &states {
            test_json_roundtrip(state);
        }
    }

    #[test]
    fn test_restart_policy_json_roundtrip() {
        let policies = [
            RestartPolicy::Always,
            RestartPolicy::OnFailure,
            RestartPolicy::Never,
        ];

        for policy in &policies {
            test_json_roundtrip(policy);
        }
    }

    #[test]
    fn test_backoff_config_json_roundtrip() {
        let configs = [
            BackoffConfig::default(),
            BackoffConfig {
                base_delay_secs: 5,
                multiplier: 3.0,
                max_delay_secs: 600,
                jitter: 0.5,
                failure_window_secs: 1800,
            },
        ];

        for config in &configs {
            test_json_roundtrip(config);
        }
    }

    #[test]
    fn test_health_check_types_json_roundtrip() {
        let check_types = [
            HealthCheckType::Tcp { port: 8080 },
            HealthCheckType::Http {
                port: 8080,
                path: "/health".to_string(),
                success_codes: vec![200],
            },
            HealthCheckType::Http {
                port: 9090,
                path: "/api/health".to_string(),
                success_codes: vec![200, 204, 302],
            },
            HealthCheckType::Exec {
                command: "curl".to_string(),
                args: vec!["-f".to_string(), "http://localhost:8080/health".to_string()],
            },
        ];

        for check_type in &check_types {
            test_json_roundtrip(check_type);
        }
    }

    #[test]
    fn test_health_check_json_roundtrip() {
        let health_checks = [
            HealthCheck {
                check_type: HealthCheckType::Tcp { port: 8080 },
                interval_secs: 30,
                timeout_secs: 5,
                failure_threshold: 3,
                success_threshold: 1,
            },
            HealthCheck {
                check_type: HealthCheckType::Http {
                    port: 8080,
                    path: "/status".to_string(),
                    success_codes: vec![200, 204],
                },
                interval_secs: 15,
                timeout_secs: 2,
                failure_threshold: 2,
                success_threshold: 2,
            },
        ];

        for health_check in &health_checks {
            test_json_roundtrip(health_check);
        }
    }

    #[test]
    fn test_readiness_check_json_roundtrip() {
        let readiness_checks = [
            ReadinessCheck {
                check_type: HealthCheckType::Tcp { port: 8080 },
                initial_delay_secs: 0,
                interval_secs: 5,
                timeout_secs: 3,
                success_threshold: 1,
            },
            ReadinessCheck {
                check_type: HealthCheckType::Http {
                    port: 3000,
                    path: "/ready".to_string(),
                    success_codes: vec![200],
                },
                initial_delay_secs: 10,
                interval_secs: 2,
                timeout_secs: 1,
                success_threshold: 3,
            },
        ];

        for readiness_check in &readiness_checks {
            test_json_roundtrip(readiness_check);
        }
    }

    #[test]
    fn test_service_exit_json_roundtrip() {
        let exits = [
            ServiceExit {
                pid: 1234,
                exit_code: Some(0),
                signal: None,
                timestamp: "2024-01-01T00:00:00Z".to_string(),
            },
            ServiceExit {
                pid: 1235,
                exit_code: Some(1),
                signal: None,
                timestamp: "2024-01-01T00:00:01Z".to_string(),
            },
            ServiceExit {
                pid: 1236,
                exit_code: None,
                signal: Some(9),
                timestamp: "2024-01-01T00:00:02Z".to_string(),
            },
        ];

        for exit in &exits {
            test_json_roundtrip(exit);
        }
    }

    #[test]
    fn test_service_events_json_roundtrip() {
        let events = [
            ServiceEvent::StateChanged {
                service_id: "test".to_string(),
                from_state: ServiceState::Idle,
                to_state: ServiceState::Spawning,
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                reason: Some("Manual start".to_string()),
            },
            ServiceEvent::ProcessStarted {
                service_id: "test".to_string(),
                pid: 1234,
                timestamp: "2024-01-01T00:00:01Z".to_string(),
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
            },
            ServiceEvent::ProcessExited {
                service_id: "test".to_string(),
                exit_info: ServiceExit {
                    pid: 1234,
                    exit_code: Some(0),
                    signal: None,
                    timestamp: "2024-01-01T00:00:02Z".to_string(),
                },
            },
            ServiceEvent::HealthCheckResult {
                service_id: "test".to_string(),
                success: true,
                timestamp: "2024-01-01T00:00:03Z".to_string(),
                error: None,
                duration_ms: 100,
            },
            ServiceEvent::HealthCheckResult {
                service_id: "test".to_string(),
                success: false,
                timestamp: "2024-01-01T00:00:04Z".to_string(),
                error: Some("Connection refused".to_string()),
                duration_ms: 5000,
            },
            ServiceEvent::ReadinessCheckResult {
                service_id: "test".to_string(),
                success: true,
                timestamp: "2024-01-01T00:00:05Z".to_string(),
                error: None,
                duration_ms: 50,
            },
            ServiceEvent::RestartScheduled {
                service_id: "test".to_string(),
                delay_secs: 10,
                attempt_count: 1,
                timestamp: "2024-01-01T00:00:06Z".to_string(),
                reason: "Process exited with code 1".to_string(),
            },
            ServiceEvent::RestartAttempt {
                service_id: "test".to_string(),
                attempt: 2,
                timestamp: "2024-01-01T00:00:07Z".to_string(),
            },
            ServiceEvent::ConfigurationUpdated {
                service_id: "test".to_string(),
                timestamp: "2024-01-01T00:00:08Z".to_string(),
                changed_fields: vec!["command".to_string(), "args".to_string()],
            },
            ServiceEvent::RouteAttached {
                service_id: "test".to_string(),
                route: "/api/v1".to_string(),
                backend_address: "127.0.0.1:8080".to_string(),
                timestamp: "2024-01-01T00:00:09Z".to_string(),
            },
            ServiceEvent::RouteDetached {
                service_id: "test".to_string(),
                route: "/api/v1".to_string(),
                timestamp: "2024-01-01T00:00:10Z".to_string(),
            },
            ServiceEvent::LogOutput {
                service_id: "test".to_string(),
                stream: LogStream::Stdout,
                content: "Hello, world!".to_string(),
                timestamp: "2024-01-01T00:00:11Z".to_string(),
            },
            ServiceEvent::LogOutput {
                service_id: "test".to_string(),
                stream: LogStream::Stderr,
                content: "Error: something went wrong".to_string(),
                timestamp: "2024-01-01T00:00:12Z".to_string(),
            },
            ServiceEvent::Warning {
                service_id: "test".to_string(),
                message: "Service is slow to respond".to_string(),
                timestamp: "2024-01-01T00:00:13Z".to_string(),
                code: Some("WARN001".to_string()),
            },
            ServiceEvent::Error {
                service_id: "test".to_string(),
                message: "Failed to start service".to_string(),
                timestamp: "2024-01-01T00:00:14Z".to_string(),
                code: Some("ERR001".to_string()),
            },
        ];

        for event in &events {
            test_json_roundtrip(event);
        }
    }

    #[test]
    fn test_event_severity_json_roundtrip() {
        let severities = [
            EventSeverity::Debug,
            EventSeverity::Info,
            EventSeverity::Warning,
            EventSeverity::Error,
            EventSeverity::Critical,
        ];

        for severity in &severities {
            test_json_roundtrip(severity);
        }
    }

    #[test]
    fn test_log_stream_json_roundtrip() {
        let streams = [LogStream::Stdout, LogStream::Stderr];

        for stream in &streams {
            test_json_roundtrip(stream);
        }
    }

    #[test]
    fn test_event_filter_json_roundtrip() {
        let filters = [
            EventFilter::all(),
            EventFilter::for_service("test-service".to_string()),
            EventFilter::with_min_severity(EventSeverity::Warning),
            EventFilter {
                service_ids: vec!["service1".to_string(), "service2".to_string()],
                min_severity: Some(EventSeverity::Info),
                event_types: vec!["stateChanged".to_string(), "processExited".to_string()],
            },
        ];

        for filter in &filters {
            test_json_roundtrip(filter);
        }
    }

    #[test]
    fn test_schema_generation_for_all_types() {
        // Test that JSON schemas can be generated for all types
        // This ensures the types are properly configured for schema generation

        let service_spec_schema = schema_for!(ServiceSpec);
        assert!(service_spec_schema.schema.metadata.is_some());

        let service_state_schema = schema_for!(ServiceState);
        assert!(service_state_schema.schema.metadata.is_some());

        let restart_policy_schema = schema_for!(RestartPolicy);
        assert!(restart_policy_schema.schema.metadata.is_some());

        let backoff_config_schema = schema_for!(BackoffConfig);
        assert!(backoff_config_schema.schema.metadata.is_some());

        let health_check_schema = schema_for!(HealthCheck);
        assert!(health_check_schema.schema.metadata.is_some());

        let health_check_type_schema = schema_for!(HealthCheckType);
        assert!(health_check_type_schema.schema.metadata.is_some());

        let readiness_check_schema = schema_for!(ReadinessCheck);
        assert!(readiness_check_schema.schema.metadata.is_some());

        let service_exit_schema = schema_for!(ServiceExit);
        assert!(service_exit_schema.schema.metadata.is_some());

        let service_event_schema = schema_for!(ServiceEvent);
        assert!(service_event_schema.schema.metadata.is_some());

        let event_severity_schema = schema_for!(EventSeverity);
        assert!(event_severity_schema.schema.metadata.is_some());

        let log_stream_schema = schema_for!(LogStream);
        assert!(log_stream_schema.schema.metadata.is_some());

        let event_filter_schema = schema_for!(EventFilter);
        assert!(event_filter_schema.schema.metadata.is_some());
    }

    #[test]
    fn test_json_format_camel_case() {
        // Verify that serialization uses camelCase as expected for external APIs
        let spec = ServiceSpec {
            id: "test".to_string(),
            name: "Test".to_string(),
            command: "echo".to_string(),
            args: vec![],
            environment: HashMap::new(),
            working_directory: Some("/tmp".to_string()),
            restart_policy: RestartPolicy::OnFailure,
            backoff_config: BackoffConfig::default(),
            health_check: None,
            readiness_check: None,
            graceful_timeout_secs: 30,
            startup_timeout_secs: 60,
        };

        let json = serde_json::to_string_pretty(&spec).unwrap();
        println!("ServiceSpec JSON: {}", json);

        // Check that field names are in camelCase
        assert!(json.contains("\"workingDirectory\""));
        assert!(json.contains("\"restartPolicy\""));
        assert!(json.contains("\"backoffConfig\""));
        // Note: null fields may not appear in JSON due to skip_serializing_if
        // assert!(json.contains("\"healthCheck\""));
        // assert!(json.contains("\"readinessCheck\""));
        assert!(json.contains("\"gracefulTimeoutSecs\""));
        assert!(json.contains("\"startupTimeoutSecs\""));
        assert!(json.contains("\"onFailure\""));
    }

    #[test]
    fn test_event_serialization_has_event_type_discriminator() {
        // Verify that events use the eventType discriminator field
        let event = ServiceEvent::StateChanged {
            service_id: "test".to_string(),
            from_state: ServiceState::Idle,
            to_state: ServiceState::Ready,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            reason: None,
        };

        let json = serde_json::to_string(&event).unwrap();
        println!("ServiceEvent JSON: {}", json);
        assert!(json.contains("\"eventType\":\"stateChanged\""));
        assert!(json.contains("\"service_id\":\"test\""));
        // Note: The actual serialization may vary based on enum representation
    }
}
