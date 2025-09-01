//! Restart policy logic and backoff calculation
//!
//! This module implements the restart decision logic and exponential backoff
//! calculation for the supervisor system. It handles different restart policies
//! and manages failure tracking within time windows.

use schema::{BackoffConfig, RestartPolicy, ServiceExit};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

/// Action to take when a service exits
#[derive(Debug, Clone, PartialEq)]
pub enum RestartAction {
    /// Restart the service after the specified delay
    Restart { delay: Duration },
    /// Do not restart the service
    Stop,
}

/// Tracks service failures within a time window for backoff calculation
#[derive(Debug, Clone)]
pub struct FailureTracker {
    /// List of failure timestamps (in seconds since Unix epoch)
    failures: Vec<u64>,
    /// Configuration for failure tracking
    window_duration: Duration,
}

impl FailureTracker {
    /// Create a new failure tracker
    pub fn new(window_duration: Duration) -> Self {
        Self {
            failures: Vec::new(),
            window_duration,
        }
    }

    /// Record a new failure
    pub fn record_failure(&mut self, timestamp: SystemTime) {
        let secs = timestamp
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        
        self.failures.push(secs);
        self.cleanup_old_failures(timestamp);
        
        debug!("Recorded failure at timestamp {}, total failures in window: {}", 
               secs, self.failures.len());
    }

    /// Clear all failures (called after a successful run period)
    pub fn reset(&mut self) {
        debug!("Resetting failure tracker, had {} failures", self.failures.len());
        self.failures.clear();
    }

    /// Get the number of failures in the current window
    pub fn failure_count(&self, current_time: SystemTime) -> usize {
        let mut tracker = self.clone();
        tracker.cleanup_old_failures(current_time);
        tracker.failures.len()
    }

    /// Remove failures outside the current window
    fn cleanup_old_failures(&mut self, current_time: SystemTime) {
        let current_secs = current_time
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        
        let window_start = current_secs.saturating_sub(self.window_duration.as_secs());
        
        self.failures.retain(|&failure_time| failure_time >= window_start);
    }
}

/// Restart policy engine that determines restart actions and calculates backoff delays
pub struct RestartPolicyEngine {
    policy: RestartPolicy,
    backoff_config: BackoffConfig,
    failure_tracker: FailureTracker,
}

impl RestartPolicyEngine {
    /// Create a new restart policy engine
    pub fn new(policy: RestartPolicy, backoff_config: BackoffConfig) -> Self {
        Self {
            policy,
            backoff_config,
            failure_tracker: FailureTracker::new(backoff_config.failure_window()),
        }
    }

    /// Determine the restart action for a service exit
    pub fn should_restart(&mut self, exit_info: &ServiceExit) -> RestartAction {
        let current_time = SystemTime::now();
        
        debug!("Evaluating restart policy {:?} for exit: pid={}, exit_code={:?}, signal={:?}",
               self.policy, exit_info.pid, exit_info.exit_code, exit_info.signal);
        
        match self.policy {
            RestartPolicy::Never => {
                debug!("RestartPolicy::Never - not restarting service");
                RestartAction::Stop
            }
            RestartPolicy::OnFailure => {
                if exit_info.is_failure() {
                    debug!("RestartPolicy::OnFailure - service failed, will restart");
                    self.failure_tracker.record_failure(current_time);
                    let delay = self.calculate_backoff_delay(current_time);
                    RestartAction::Restart { delay }
                } else {
                    debug!("RestartPolicy::OnFailure - service succeeded, not restarting");
                    // Success - reset failure tracking for future
                    self.failure_tracker.reset();
                    RestartAction::Stop
                }
            }
            RestartPolicy::Always => {
                if exit_info.is_failure() {
                    debug!("RestartPolicy::Always - service failed, will restart with backoff");
                    self.failure_tracker.record_failure(current_time);
                } else {
                    debug!("RestartPolicy::Always - service succeeded, will restart immediately");
                    // Success - reset failure tracking but still restart
                    self.failure_tracker.reset();
                }
                let delay = self.calculate_backoff_delay(current_time);
                RestartAction::Restart { delay }
            }
        }
    }

    /// Calculate the backoff delay based on failure history
    fn calculate_backoff_delay(&self, current_time: SystemTime) -> Duration {
        let failure_count = self.failure_tracker.failure_count(current_time);
        
        if failure_count == 0 {
            debug!("No recent failures, using minimum delay");
            return Duration::from_millis(100); // Minimum delay for immediate restarts
        }

        // Calculate exponential backoff: base_delay * multiplier^(failures-1)
        let base_delay_ms = self.backoff_config.base_delay().as_millis() as f64;
        let multiplier = self.backoff_config.multiplier;
        let exponent = (failure_count - 1) as f64;
        
        let calculated_delay_ms = base_delay_ms * multiplier.powf(exponent);
        let max_delay_ms = self.backoff_config.max_delay().as_millis() as f64;
        
        // Cap at maximum delay
        let capped_delay_ms = calculated_delay_ms.min(max_delay_ms);
        
        // Apply jitter
        let jitter_factor = 1.0 + (self.backoff_config.jitter * (2.0 * random_f64() - 1.0));
        let final_delay_ms = (capped_delay_ms * jitter_factor).max(0.0);
        
        let delay = Duration::from_millis(final_delay_ms as u64);
        
        debug!("Calculated backoff delay: {} failures -> {}ms (base={}ms, multiplier={}, max={}ms, jitter={})",
               failure_count, final_delay_ms, base_delay_ms, multiplier, max_delay_ms, self.backoff_config.jitter);
        
        delay
    }

    /// Reset the failure tracker (called after a period of successful operation)
    pub fn reset_failures(&mut self) {
        self.failure_tracker.reset();
    }

    /// Get the current failure count
    pub fn current_failure_count(&self) -> usize {
        self.failure_tracker.failure_count(SystemTime::now())
    }
}

/// Simple pseudo-random number generator for jitter
/// We use a simple approach to avoid adding external dependencies
fn random_f64() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    
    static SEED: AtomicU64 = AtomicU64::new(1);
    
    // Linear congruential generator
    let prev = SEED.load(Ordering::Relaxed);
    let next = prev.wrapping_mul(1103515245).wrapping_add(12345);
    SEED.store(next, Ordering::Relaxed);
    
    // Convert to [0, 1) range
    (next as f64) / (u64::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn create_test_backoff_config() -> BackoffConfig {
        BackoffConfig {
            base_delay_secs: 1,
            multiplier: 2.0,
            max_delay_secs: 60,
            jitter: 0.1,
            failure_window_secs: 300, // 5 minutes
        }
    }

    fn create_exit_success() -> ServiceExit {
        ServiceExit {
            pid: 1234,
            exit_code: Some(0),
            signal: None,
            timestamp: "2024-01-01T12:00:00Z".to_string(),
        }
    }

    fn create_exit_failure() -> ServiceExit {
        ServiceExit {
            pid: 1234,
            exit_code: Some(1),
            signal: None,
            timestamp: "2024-01-01T12:00:00Z".to_string(),
        }
    }

    fn create_exit_signal() -> ServiceExit {
        ServiceExit {
            pid: 1234,
            exit_code: None,
            signal: Some(9), // SIGKILL
            timestamp: "2024-01-01T12:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_restart_policy_never() {
        let mut engine = RestartPolicyEngine::new(
            RestartPolicy::Never,
            create_test_backoff_config(),
        );

        // Should never restart, regardless of exit status
        let action = engine.should_restart(&create_exit_success());
        assert_eq!(action, RestartAction::Stop);

        let action = engine.should_restart(&create_exit_failure());
        assert_eq!(action, RestartAction::Stop);

        let action = engine.should_restart(&create_exit_signal());
        assert_eq!(action, RestartAction::Stop);
    }

    #[test]
    fn test_restart_policy_on_failure() {
        let mut engine = RestartPolicyEngine::new(
            RestartPolicy::OnFailure,
            create_test_backoff_config(),
        );

        // Should not restart on success
        let action = engine.should_restart(&create_exit_success());
        assert_eq!(action, RestartAction::Stop);

        // Should restart on failure
        let action = engine.should_restart(&create_exit_failure());
        assert!(matches!(action, RestartAction::Restart { .. }));

        // Should restart on signal
        let action = engine.should_restart(&create_exit_signal());
        assert!(matches!(action, RestartAction::Restart { .. }));
    }

    #[test]
    fn test_restart_policy_always() {
        let mut engine = RestartPolicyEngine::new(
            RestartPolicy::Always,
            create_test_backoff_config(),
        );

        // Should always restart
        let action = engine.should_restart(&create_exit_success());
        assert!(matches!(action, RestartAction::Restart { .. }));

        let action = engine.should_restart(&create_exit_failure());
        assert!(matches!(action, RestartAction::Restart { .. }));

        let action = engine.should_restart(&create_exit_signal());
        assert!(matches!(action, RestartAction::Restart { .. }));
    }

    #[test]
    fn test_exponential_backoff() {
        let config = BackoffConfig {
            base_delay_secs: 1,
            multiplier: 2.0,
            max_delay_secs: 8,
            jitter: 0.0, // No jitter for predictable testing
            failure_window_secs: 300,
        };

        let mut engine = RestartPolicyEngine::new(RestartPolicy::OnFailure, config);

        // First failure - should use base delay
        let action1 = engine.should_restart(&create_exit_failure());
        if let RestartAction::Restart { delay } = action1 {
            // Should be approximately 1 second (base delay)
            assert!(delay.as_millis() >= 900 && delay.as_millis() <= 1100);
        } else {
            panic!("Expected restart action");
        }

        // Second failure - should double
        let action2 = engine.should_restart(&create_exit_failure());
        if let RestartAction::Restart { delay } = action2 {
            // Should be approximately 2 seconds
            assert!(delay.as_millis() >= 1900 && delay.as_millis() <= 2100);
        } else {
            panic!("Expected restart action");
        }

        // Third failure - should double again  
        let action3 = engine.should_restart(&create_exit_failure());
        if let RestartAction::Restart { delay } = action3 {
            // Should be approximately 4 seconds
            assert!(delay.as_millis() >= 3900 && delay.as_millis() <= 4100);
        } else {
            panic!("Expected restart action");
        }

        // Fourth failure - should be capped at max_delay (8 seconds)
        let action4 = engine.should_restart(&create_exit_failure());
        if let RestartAction::Restart { delay } = action4 {
            // Should be capped at 8 seconds
            assert!(delay.as_millis() >= 7900 && delay.as_millis() <= 8100);
        } else {
            panic!("Expected restart action");
        }
    }

    #[test]
    fn test_failure_tracker_window() {
        let window_duration = Duration::from_secs(10);
        let mut tracker = FailureTracker::new(window_duration);

        let base_time = SystemTime::now();
        
        // Add a failure at base time
        tracker.record_failure(base_time);
        assert_eq!(tracker.failure_count(base_time), 1);

        // Add a failure 5 seconds later
        let later_time = base_time + Duration::from_secs(5);
        tracker.record_failure(later_time);
        assert_eq!(tracker.failure_count(later_time), 2);

        // Check count 15 seconds from base (first failure should be outside window)
        let much_later_time = base_time + Duration::from_secs(15);
        assert_eq!(tracker.failure_count(much_later_time), 1);

        // Check count 20 seconds from base (both failures should be outside window)
        let very_late_time = base_time + Duration::from_secs(20);
        assert_eq!(tracker.failure_count(very_late_time), 0);
    }

    #[test]
    fn test_failure_tracker_reset() {
        let mut tracker = FailureTracker::new(Duration::from_secs(300));
        
        // Add some failures
        let now = SystemTime::now();
        tracker.record_failure(now);
        tracker.record_failure(now);
        assert_eq!(tracker.failure_count(now), 2);

        // Reset should clear all failures
        tracker.reset();
        assert_eq!(tracker.failure_count(now), 0);
    }

    #[test]
    fn test_jitter_application() {
        let config = BackoffConfig {
            base_delay_secs: 2,
            multiplier: 1.0, // No exponential growth
            max_delay_secs: 60,
            jitter: 0.5, // 50% jitter
            failure_window_secs: 300,
        };

        let mut engine = RestartPolicyEngine::new(RestartPolicy::OnFailure, config);

        // Get multiple delays and verify they have jitter applied
        let mut delays = Vec::new();
        for _ in 0..10 {
            let action = engine.should_restart(&create_exit_failure());
            if let RestartAction::Restart { delay } = action {
                delays.push(delay.as_millis());
            }
        }

        // All delays should be different due to jitter
        // And should be within reasonable bounds (base_delay ± jitter)
        let base_delay_ms = 2000; // 2 seconds
        for &delay_ms in &delays {
            // With 50% jitter, delay should be between 1s and 3s
            assert!(delay_ms >= 1000 && delay_ms <= 3000, "Delay {} outside expected range", delay_ms);
        }

        // Check that we actually have variation (not all the same)
        let first_delay = delays[0];
        let has_variation = delays.iter().any(|&d| d != first_delay);
        assert!(has_variation, "Expected variation in delays due to jitter");
    }
}
