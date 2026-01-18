//! Integration tests for supervisor restart policy behavior
//!
//! These tests verify that the supervisor correctly applies restart policies
//! and backoff logic when processes exit.

use crate::proxy::NoopProxyAdapter;
use crate::supervisor::{spawn_supervisor, MockInstruction, MockProcessAdapter, SupervisorConfig};
use schema::{BackoffConfig, RestartPolicy, ServiceEvent, ServiceSpec, ServiceState};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::{sleep, timeout};
use tracing::debug;

fn create_test_spec(restart_policy: RestartPolicy, backoff_config: BackoffConfig) -> ServiceSpec {
    ServiceSpec {
        id: "restart-test-service".to_string(),
        name: "Restart Test Service".to_string(),
        command: "echo".to_string(),
        args: vec!["test".to_string()],
        environment: HashMap::default(),
        working_directory: None,
        route: None,
        restart_policy,
        backoff_config,
        health_check: None,
        readiness_check: None,
        graceful_timeout_secs: 1,
        startup_timeout_secs: 5,
    }
}

async fn collect_events(
    mut event_rx: broadcast::Receiver<ServiceEvent>,
    timeout_duration: Duration,
) -> Vec<ServiceEvent> {
    let mut events = Vec::new();

    while let Ok(Ok(event)) = timeout(timeout_duration, event_rx.recv()).await {
        events.push(event);
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_restart_policy_never() {
        let spec = create_test_spec(
            RestartPolicy::Never,
            BackoffConfig {
                base_delay_secs: 1,
                multiplier: 2.0,
                max_delay_secs: 60,
                jitter: 0.0,
                failure_window_secs: 300,
            },
        );

        // Create mock adapter that will cause the process to fail
        let process_adapter = Arc::new(MockProcessAdapter::new());
        process_adapter
            .add_instruction(MockInstruction {
                exit_delay: Duration::from_millis(50),
                exit_code: Some(1), // Failure
                signal: None,
                responds_to_signals: true,
            })
            .await;

        let (event_tx, event_rx) = broadcast::channel(100);

        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
            proxy_adapter: Arc::new(NoopProxyAdapter),
        };

        let handle = spawn_supervisor(config);

        // Start the service
        handle.start().unwrap();

        // Wait for events
        let events = collect_events(event_rx, Duration::from_millis(500)).await;

        // Should see: StateChanged(Idle->Spawning), ProcessStarted, StateChanged(Spawning->Ready), ProcessExited, StateChanged(Ready->Idle)
        // But NO restart because policy is Never
        let state_changes: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ServiceEvent::StateChanged {
                    from_state,
                    to_state,
                    ..
                } => Some((from_state, to_state)),
                _ => None,
            })
            .collect();

        // Should see transitions to Idle and stay there (no restart)
        assert!(state_changes
            .iter()
            .any(|(_, to)| **to == ServiceState::Spawning));
        assert!(state_changes
            .iter()
            .any(|(_, to)| **to == ServiceState::Idle));

        // Should not see multiple spawning events (indicating no restart)
        let spawning_count = state_changes
            .iter()
            .filter(|(_, to)| **to == ServiceState::Spawning)
            .count();
        assert_eq!(
            spawning_count, 1,
            "Should only see one spawning transition with Never policy"
        );

        // Current state should be Idle
        assert_eq!(handle.current_state(), ServiceState::Idle);

        handle.shutdown().unwrap();
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_restart_policy_on_failure_with_success() {
        let spec = create_test_spec(
            RestartPolicy::OnFailure,
            BackoffConfig {
                base_delay_secs: 1,
                multiplier: 2.0,
                max_delay_secs: 60,
                jitter: 0.0,
                failure_window_secs: 300,
            },
        );

        let process_adapter = Arc::new(MockProcessAdapter::new());
        // First process succeeds - should not restart
        process_adapter
            .add_instruction(MockInstruction {
                exit_delay: Duration::from_millis(50),
                exit_code: Some(0), // Success
                signal: None,
                responds_to_signals: true,
            })
            .await;

        let (event_tx, event_rx) = broadcast::channel(100);

        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
            proxy_adapter: Arc::new(NoopProxyAdapter),
        };

        let handle = spawn_supervisor(config);
        handle.start().unwrap();

        let events = collect_events(event_rx, Duration::from_millis(500)).await;

        let state_changes: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ServiceEvent::StateChanged { to_state, .. } => Some(*to_state),
                _ => None,
            })
            .collect();

        // Should see spawning once and end in idle (no restart for success)
        let spawning_count = state_changes
            .iter()
            .filter(|&state| *state == ServiceState::Spawning)
            .count();
        assert_eq!(
            spawning_count, 1,
            "Should only see one spawning transition for successful exit"
        );

        assert_eq!(handle.current_state(), ServiceState::Idle);

        handle.shutdown().unwrap();
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_restart_policy_on_failure_with_failure() {
        let spec = create_test_spec(
            RestartPolicy::OnFailure,
            BackoffConfig {
                base_delay_secs: 1,
                multiplier: 1.0, // No exponential growth for simpler testing
                max_delay_secs: 60,
                jitter: 0.0,
                failure_window_secs: 300,
            },
        );

        let process_adapter = Arc::new(MockProcessAdapter::new());
        // First process fails - should restart
        process_adapter
            .add_instruction(MockInstruction {
                exit_delay: Duration::from_millis(50),
                exit_code: Some(1), // Failure
                signal: None,
                responds_to_signals: true,
            })
            .await;
        // Second process succeeds - should not restart again
        process_adapter
            .add_instruction(MockInstruction {
                exit_delay: Duration::from_millis(50),
                exit_code: Some(0), // Success
                signal: None,
                responds_to_signals: true,
            })
            .await;

        let (event_tx, event_rx) = broadcast::channel(100);

        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
            proxy_adapter: Arc::new(NoopProxyAdapter),
        };

        let handle = spawn_supervisor(config);
        handle.start().unwrap();

        // Wait longer to see the restart sequence
        let events = collect_events(event_rx, Duration::from_secs(2)).await;

        let state_changes: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ServiceEvent::StateChanged { to_state, .. } => Some(*to_state),
                _ => None,
            })
            .collect();

        // Should see at least two spawning events (initial + restart)
        let spawning_count = state_changes
            .iter()
            .filter(|&state| *state == ServiceState::Spawning)
            .count();
        assert!(
            spawning_count >= 2,
            "Should see at least 2 spawning transitions (initial + restart), got {spawning_count}"
        );

        handle.shutdown().unwrap();
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_restart_policy_always() {
        let spec = create_test_spec(
            RestartPolicy::Always,
            BackoffConfig {
                base_delay_secs: 1,
                multiplier: 1.0,
                max_delay_secs: 60,
                jitter: 0.0,
                failure_window_secs: 300,
            },
        );

        let process_adapter = Arc::new(MockProcessAdapter::new());
        // Add a few specific instructions
        process_adapter
            .add_instruction(MockInstruction {
                exit_delay: Duration::from_millis(50),
                exit_code: Some(0), // Success - should still restart with Always policy
                signal: None,
                responds_to_signals: true,
            })
            .await;
        process_adapter
            .add_instruction(MockInstruction {
                exit_delay: Duration::from_millis(50),
                exit_code: Some(1), // Failure - should restart with Always policy
                signal: None,
                responds_to_signals: true,
            })
            .await;

        let (event_tx, mut event_rx) = broadcast::channel(100);

        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
            proxy_adapter: Arc::new(NoopProxyAdapter),
        };

        let handle = spawn_supervisor(config);
        handle.start().unwrap();

        // Collect events for a limited time, counting spawning events
        let mut spawning_count = 0;
        let start_time = tokio::time::Instant::now();
        let timeout_duration = Duration::from_secs(3);

        while start_time.elapsed() < timeout_duration {
            match timeout(Duration::from_millis(100), event_rx.recv()).await {
                Ok(Ok(event)) => {
                    if let ServiceEvent::StateChanged {
                        to_state: ServiceState::Spawning,
                        ..
                    } = event
                    {
                        spawning_count += 1;
                        // Stop after we see enough spawning events to prove the Always policy works
                        if spawning_count >= 3 {
                            break;
                        }
                    }
                }
                Ok(Err(_)) => break, // Channel closed
                Err(_) => {}         // Continue on timeout
            }
        }

        // Shut down the supervisor
        handle.shutdown().unwrap();
        sleep(Duration::from_millis(100)).await;

        // Should see multiple spawning events (restarts for both success and failure)
        assert!(
            spawning_count >= 2,
            "Should see at least 2 spawning transitions with Always policy, got {spawning_count}"
        );
    }

    #[tokio::test]
    async fn test_exponential_backoff_timing() {
        let spec = create_test_spec(
            RestartPolicy::OnFailure,
            BackoffConfig {
                base_delay_secs: 1, // 1 second base delay
                multiplier: 2.0,
                max_delay_secs: 8,
                jitter: 0.0, // No jitter for predictable timing
                failure_window_secs: 300,
            },
        );

        let process_adapter = Arc::new(MockProcessAdapter::new());
        // Add multiple failures to test backoff
        for _ in 0..3 {
            process_adapter
                .add_instruction(MockInstruction {
                    exit_delay: Duration::from_millis(50),
                    exit_code: Some(1), // Failure
                    signal: None,
                    responds_to_signals: true,
                })
                .await;
        }

        let (event_tx, event_rx) = broadcast::channel(100);

        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
            proxy_adapter: Arc::new(NoopProxyAdapter),
        };

        let handle = spawn_supervisor(config);
        let start_time = tokio::time::Instant::now();

        handle.start().unwrap();

        // Wait long enough to see multiple restart cycles with backoff
        let events = collect_events(event_rx, Duration::from_secs(10)).await;

        let elapsed = start_time.elapsed();

        // Count process started events (each represents a new attempt)
        let process_starts = events
            .iter()
            .filter(|event| matches!(event, ServiceEvent::ProcessStarted { .. }))
            .count();

        debug!("Saw {} process starts in {:?}", process_starts, elapsed);

        // Should see multiple process starts with increasing delays between them
        assert!(
            process_starts >= 2,
            "Should see at least 2 process starts with backoff"
        );

        // The total elapsed time should reflect the backoff delays
        // 1st restart after ~1s, 2nd restart after ~2s more, etc.
        assert!(
            elapsed.as_secs() >= 3,
            "Should take at least 3 seconds with exponential backoff"
        );

        handle.shutdown().unwrap();
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_manual_stop_cancels_restart() {
        let spec = create_test_spec(
            RestartPolicy::Always,
            BackoffConfig {
                base_delay_secs: 2, // 2 second delay
                multiplier: 1.0,
                max_delay_secs: 60,
                jitter: 0.0,
                failure_window_secs: 300,
            },
        );

        let process_adapter = Arc::new(MockProcessAdapter::new());
        process_adapter
            .add_instruction(MockInstruction {
                exit_delay: Duration::from_millis(50),
                exit_code: Some(1), // Failure - would normally restart
                signal: None,
                responds_to_signals: true,
            })
            .await;

        let (event_tx, event_rx) = broadcast::channel(100);

        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
            proxy_adapter: Arc::new(NoopProxyAdapter),
        };

        let handle = spawn_supervisor(config);
        handle.start().unwrap();

        // Wait for initial process to start and fail
        sleep(Duration::from_millis(200)).await;

        // Stop the service during the restart delay
        handle.stop().unwrap();

        // Collect events for a while to see if restart still happens
        let events = collect_events(event_rx, Duration::from_secs(3)).await;

        let process_starts = events
            .iter()
            .filter(|event| matches!(event, ServiceEvent::ProcessStarted { .. }))
            .count();

        // Should only see one process start (initial), not a restart
        assert_eq!(
            process_starts, 1,
            "Manual stop should prevent restart, but saw {process_starts} process starts"
        );

        assert_eq!(handle.current_state(), ServiceState::Idle);

        handle.shutdown().unwrap();
        sleep(Duration::from_millis(100)).await;
    }
}
