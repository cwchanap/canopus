//! Comprehensive tests for restart policy logic
//!
//! This module contains table-driven tests that verify restart policy behavior
//! across different scenarios, exit codes, and backoff configurations.

use crate::supervisor::restart_policy::{RestartPolicyEngine, RestartAction};
use schema::{RestartPolicy, BackoffConfig, ServiceExit};
use std::time::SystemTime;

/// Test case for restart policy evaluation
struct RestartPolicyTestCase {
    name: &'static str,
    policy: RestartPolicy,
    backoff_config: BackoffConfig,
    exits: Vec<TestExit>,
    expected_actions: Vec<ExpectedAction>,
}

/// Test exit information
struct TestExit {
    exit_code: Option<i32>,
    signal: Option<i32>,
}

/// Expected restart action
struct ExpectedAction {
    should_restart: bool,
    delay_min_ms: u64,
    delay_max_ms: u64,
}

impl TestExit {
    fn success() -> Self {
        Self { exit_code: Some(0), signal: None }
    }

    fn failure(code: i32) -> Self {
        Self { exit_code: Some(code), signal: None }
    }

    fn signal(sig: i32) -> Self {
        Self { exit_code: None, signal: Some(sig) }
    }

    fn to_service_exit(&self, pid: u32) -> ServiceExit {
        ServiceExit {
            pid,
            exit_code: self.exit_code,
            signal: self.signal,
            timestamp: "2024-01-01T12:00:00Z".to_string(),
        }
    }
}

impl ExpectedAction {
    fn stop() -> Self {
        Self { should_restart: false, delay_min_ms: 0, delay_max_ms: 0 }
    }

    fn restart(delay_min_ms: u64, delay_max_ms: u64) -> Self {
        Self { should_restart: true, delay_min_ms, delay_max_ms }
    }

    fn restart_exact(delay_ms: u64) -> Self {
        Self { should_restart: true, delay_min_ms: delay_ms, delay_max_ms: delay_ms }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_cases() -> Vec<RestartPolicyTestCase> {
        vec![
            // Test case: RestartPolicy::Never
            RestartPolicyTestCase {
                name: "Never policy - should never restart",
                policy: RestartPolicy::Never,
                backoff_config: BackoffConfig {
                    base_delay_secs: 1,
                    multiplier: 2.0,
                    max_delay_secs: 60,
                    jitter: 0.0,
                    failure_window_secs: 300,
                },
                exits: vec![
                    TestExit::success(),
                    TestExit::failure(1),
                    TestExit::signal(9),
                ],
                expected_actions: vec![
                    ExpectedAction::stop(),
                    ExpectedAction::stop(),
                    ExpectedAction::stop(),
                ],
            },

            // Test case: RestartPolicy::OnFailure - successes
            RestartPolicyTestCase {
                name: "OnFailure policy - success cases",
                policy: RestartPolicy::OnFailure,
                backoff_config: BackoffConfig {
                    base_delay_secs: 1,
                    multiplier: 2.0,
                    max_delay_secs: 60,
                    jitter: 0.0,
                    failure_window_secs: 300,
                },
                exits: vec![
                    TestExit::success(),
                    TestExit::success(),
                ],
                expected_actions: vec![
                    ExpectedAction::stop(),
                    ExpectedAction::stop(),
                ],
            },

            // Test case: RestartPolicy::OnFailure - failures with exponential backoff
            RestartPolicyTestCase {
                name: "OnFailure policy - exponential backoff",
                policy: RestartPolicy::OnFailure,
                backoff_config: BackoffConfig {
                    base_delay_secs: 1,
                    multiplier: 2.0,
                    max_delay_secs: 8,
                    jitter: 0.0,
                    failure_window_secs: 300,
                },
                exits: vec![
                    TestExit::failure(1),    // 1st failure: 1s
                    TestExit::failure(1),    // 2nd failure: 2s
                    TestExit::failure(1),    // 3rd failure: 4s
                    TestExit::failure(1),    // 4th failure: 8s (capped)
                    TestExit::failure(1),    // 5th failure: 8s (capped)
                ],
                expected_actions: vec![
                    ExpectedAction::restart_exact(1000),
                    ExpectedAction::restart_exact(2000),
                    ExpectedAction::restart_exact(4000),
                    ExpectedAction::restart_exact(8000),
                    ExpectedAction::restart_exact(8000),
                ],
            },

            // Test case: RestartPolicy::Always - mixed exits
            RestartPolicyTestCase {
                name: "Always policy - mixed exit types",
                policy: RestartPolicy::Always,
                backoff_config: BackoffConfig {
                    base_delay_secs: 2,
                    multiplier: 1.5,
                    max_delay_secs: 30,
                    jitter: 0.0,
                    failure_window_secs: 300,
                },
                exits: vec![
                    TestExit::success(),     // Success: minimal delay
                    TestExit::failure(1),    // 1st failure: 2s
                    TestExit::signal(15),    // 2nd failure: 3s
                    TestExit::success(),     // Success: reset + minimal delay
                    TestExit::failure(1),    // 1st failure again: 2s
                ],
                expected_actions: vec![
                    ExpectedAction::restart(90, 110),   // Minimal delay ~100ms
                    ExpectedAction::restart_exact(2000), // Base delay
                    ExpectedAction::restart_exact(3000), // 2s * 1.5
                    ExpectedAction::restart(90, 110),   // Reset + minimal delay
                    ExpectedAction::restart_exact(2000), // Base delay again
                ],
            },

            // Test case: Jitter application
            RestartPolicyTestCase {
                name: "Jitter application",
                policy: RestartPolicy::OnFailure,
                backoff_config: BackoffConfig {
                    base_delay_secs: 4,
                    multiplier: 1.0, // No exponential growth
                    max_delay_secs: 60,
                    jitter: 0.25, // 25% jitter
                    failure_window_secs: 300,
                },
                exits: vec![
                    TestExit::failure(1),
                    TestExit::failure(1),
                    TestExit::failure(1),
                ],
                expected_actions: vec![
                    // 4s base ± 25% = 3s to 5s
                    ExpectedAction::restart(3000, 5000),
                    ExpectedAction::restart(3000, 5000),
                    ExpectedAction::restart(3000, 5000),
                ],
            },

            // Test case: Signal-based exits
            RestartPolicyTestCase {
                name: "Signal-based exits",
                policy: RestartPolicy::OnFailure,
                backoff_config: BackoffConfig {
                    base_delay_secs: 1,
                    multiplier: 2.0,
                    max_delay_secs: 60,
                    jitter: 0.0,
                    failure_window_secs: 300,
                },
                exits: vec![
                    TestExit::signal(9),  // SIGKILL - failure
                    TestExit::signal(15), // SIGTERM - failure  
                    TestExit::signal(2),  // SIGINT - failure
                ],
                expected_actions: vec![
                    ExpectedAction::restart_exact(1000),
                    ExpectedAction::restart_exact(2000),
                    ExpectedAction::restart_exact(4000),
                ],
            },
        ]
    }

    #[test]
    fn test_restart_policy_table_driven() {
        let test_cases = create_test_cases();

        for test_case in test_cases {
            println!("Running test case: {}", test_case.name);
            
            let mut engine = RestartPolicyEngine::new(
                test_case.policy,
                test_case.backoff_config,
            );

            assert_eq!(test_case.exits.len(), test_case.expected_actions.len(),
                "Test case '{}': exits and expected actions length mismatch", test_case.name);

            for (i, (exit, expected)) in test_case.exits.iter().zip(test_case.expected_actions.iter()).enumerate() {
                let service_exit = exit.to_service_exit(1000 + i as u32);
                let action = engine.should_restart(&service_exit);

                match (&action, expected.should_restart) {
                    (RestartAction::Stop, false) => {
                        // Expected stop - good
                        println!("  Exit {}: Stop (expected)", i);
                    }
                    (RestartAction::Restart { delay }, true) => {
                        let delay_ms = delay.as_millis() as u64;
                        
                        if delay_ms >= expected.delay_min_ms && delay_ms <= expected.delay_max_ms {
                            println!("  Exit {}: Restart with {}ms delay (expected range: {}-{}ms)",
                                   i, delay_ms, expected.delay_min_ms, expected.delay_max_ms);
                        } else {
                            panic!("Test case '{}', exit {}: Restart delay {}ms outside expected range {}-{}ms",
                                  test_case.name, i, delay_ms, expected.delay_min_ms, expected.delay_max_ms);
                        }
                    }
                    (RestartAction::Stop, true) => {
                        panic!("Test case '{}', exit {}: Expected restart but got stop", test_case.name, i);
                    }
                    (RestartAction::Restart { delay }, false) => {
                        panic!("Test case '{}', exit {}: Expected stop but got restart with {:?} delay", 
                              test_case.name, i, delay);
                    }
                }
            }
        }
    }

    #[test]
    fn test_failure_window_behavior() {
        let mut engine = RestartPolicyEngine::new(
            RestartPolicy::OnFailure,
            BackoffConfig {
                base_delay_secs: 2,
                multiplier: 2.0,
                max_delay_secs: 60,
                jitter: 0.0,
                failure_window_secs: 10, // 10 second window
            },
        );

        // Create exits at specific timestamps to test windowing
        let _base_time = SystemTime::now();
        
        // First failure at time 0
        let exit1 = ServiceExit {
            pid: 1001,
            exit_code: Some(1),
            signal: None,
            timestamp: "2024-01-01T12:00:00Z".to_string(),
        };
        
        let action1 = engine.should_restart(&exit1);
        if let RestartAction::Restart { delay } = action1 {
            assert_eq!(delay.as_secs(), 2); // Base delay
        } else {
            panic!("Expected restart for first failure");
        }

        // Simulate time passing by directly testing the failure tracker
        assert_eq!(engine.current_failure_count(), 1);

        // Second failure - should see exponential backoff
        let exit2 = ServiceExit {
            pid: 1002,
            exit_code: Some(1),
            signal: None,
            timestamp: "2024-01-01T12:00:05Z".to_string(),
        };
        
        let action2 = engine.should_restart(&exit2);
        if let RestartAction::Restart { delay } = action2 {
            assert_eq!(delay.as_secs(), 4); // 2 * 2 = 4 seconds
        } else {
            panic!("Expected restart for second failure");
        }

        assert_eq!(engine.current_failure_count(), 2);
    }

    #[test]
    fn test_failure_counter_reset_on_success() {
        let mut engine = RestartPolicyEngine::new(
            RestartPolicy::Always,
            BackoffConfig {
                base_delay_secs: 1,
                multiplier: 2.0,
                max_delay_secs: 60,
                jitter: 0.0,
                failure_window_secs: 300,
            },
        );

        // First failure
        let failure1 = TestExit::failure(1).to_service_exit(1001);
        let action1 = engine.should_restart(&failure1);
        assert!(matches!(action1, RestartAction::Restart { .. }));
        assert_eq!(engine.current_failure_count(), 1);

        // Second failure - should see backoff
        let failure2 = TestExit::failure(1).to_service_exit(1002);
        let action2 = engine.should_restart(&failure2);
        if let RestartAction::Restart { delay } = action2 {
            assert!(delay.as_millis() >= 2000); // Should be doubled
        }
        assert_eq!(engine.current_failure_count(), 2);

        // Success - should reset failure count
        let success = TestExit::success().to_service_exit(1003);
        let action3 = engine.should_restart(&success);
        if let RestartAction::Restart { delay } = action3 {
            assert!(delay.as_millis() <= 200); // Should be minimal delay
        }
        assert_eq!(engine.current_failure_count(), 0);

        // Next failure should start fresh
        let failure3 = TestExit::failure(1).to_service_exit(1004);
        let action4 = engine.should_restart(&failure3);
        if let RestartAction::Restart { delay } = action4 {
            assert_eq!(delay.as_secs(), 1); // Back to base delay
        }
        assert_eq!(engine.current_failure_count(), 1);
    }

    #[test]
    fn test_backoff_cap() {
        let mut engine = RestartPolicyEngine::new(
            RestartPolicy::OnFailure,
            BackoffConfig {
                base_delay_secs: 1,
                multiplier: 10.0, // Large multiplier
                max_delay_secs: 5,  // Low cap
                jitter: 0.0,
                failure_window_secs: 300,
            },
        );

        // Cause several failures to hit the cap
        let delays = (0..5).map(|i| {
            let exit = TestExit::failure(1).to_service_exit(1000 + i);
            let action = engine.should_restart(&exit);
            match action {
                RestartAction::Restart { delay } => delay.as_secs(),
                RestartAction::Stop => panic!("Unexpected stop"),
            }
        }).collect::<Vec<_>>();

        // First delay should be base (1s)
        assert_eq!(delays[0], 1);
        
        // All subsequent delays should be capped at 5s
        for &delay in &delays[1..] {
            assert!(delay <= 5, "Delay {} exceeds cap of 5s", delay);
        }
        
        // Last few should definitely be at the cap
        assert_eq!(delays[3], 5);
        assert_eq!(delays[4], 5);
    }

    #[test]
    fn test_zero_jitter() {
        let mut engine = RestartPolicyEngine::new(
            RestartPolicy::OnFailure,
            BackoffConfig {
                base_delay_secs: 3,
                multiplier: 1.0,
                max_delay_secs: 60,
                jitter: 0.0, // No jitter
                failure_window_secs: 300,
            },
        );

        // All delays should be exactly the same with no jitter
        let delays = (0..5).map(|i| {
            let exit = TestExit::failure(1).to_service_exit(1000 + i);
            let action = engine.should_restart(&exit);
            match action {
                RestartAction::Restart { delay } => delay.as_millis(),
                RestartAction::Stop => panic!("Unexpected stop"),
            }
        }).collect::<Vec<_>>();

        // All delays should be exactly 3000ms
        for &delay in &delays {
            assert_eq!(delay, 3000, "Expected 3000ms, got {}ms", delay);
        }
    }
}
