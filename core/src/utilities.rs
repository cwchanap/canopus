//! Utility functions and helper types for core functionality

use serde_json;
use tracing::info;

/// Initialize tracing for the application
pub fn init_tracing() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("Tracing initialized");
}

/// Validate configuration data
///
/// # Errors
///
/// Returns a validation error if the input is empty or contains invalid JSON.
pub fn validate_config_data(data: &str) -> crate::Result<()> {
    if data.is_empty() {
        return Err(crate::CoreError::ValidationError(
            "Configuration data cannot be empty".to_string(),
        ));
    }

    // Try to parse as JSON to validate structure
    match serde_json::from_str::<serde_json::Value>(data) {
        Ok(_) => Ok(()),
        Err(e) => Err(crate::CoreError::ValidationError(format!(
            "Invalid JSON: {e}"
        ))),
    }
}

/// Common result type for utilities
pub type UtilityResult<T> = Result<T, crate::CoreError>;

/// Simple pseudo-random number generator (linear congruential generator).
///
/// Avoids adding external RNG dependencies. Suitable for non-cryptographic
/// uses like jitter and mock PIDs but NOT for security-sensitive contexts.
///
/// Note: The LCG constants used (1,103,515,245 and 12,345) are the classic
/// 32-bit constants from Numerical Recipes. Applied to 64-bit state, the
/// statistical quality is low but adequate for jitter and mock PIDs.
pub mod simple_rng {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Mutex-wrapped Option<AtomicU64> for atomic initialization with test reset capability.
    /// Using a single atomic guard ensures the seed value is fully visible to readers
    /// once the Option is Some - no intermediate state where "initialized" is true but
    /// the seed is stale. The Mutex ensures only one thread performs initialization.
    static SEED: Mutex<Option<AtomicU64>> = Mutex::new(None);
    static FALLBACK_COUNTER: AtomicU64 = AtomicU64::new(1);

    /// Initialize the RNG with a deterministic seed (useful for testing).
    ///
    /// Returns `true` if the seed was accepted, `false` if the RNG was already
    /// initialized (by a prior call to this function or by lazy initialization).
    /// Only the first caller wins; subsequent calls are no-ops.
    ///
    /// # Arguments
    ///
    /// * `seed` - The seed value to use. All `u64` values including 0 are valid.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned by another thread.
    ///
    /// # Examples
    ///
    /// ```
    /// use canopus_core::utilities::simple_rng::init_seed_with_value;
    ///
    /// // For reproducible test results
    /// assert!(init_seed_with_value(42));
    /// let val1 = canopus_core::utilities::simple_rng::next_u64();
    /// let val2 = canopus_core::utilities::simple_rng::next_u64();
    /// ```
    pub fn init_seed_with_value(seed: u64) -> bool {
        let mut guard = SEED.lock().unwrap();
        if guard.is_some() {
            // Already initialized, another thread beat us or already set
            false
        } else {
            *guard = Some(AtomicU64::new(seed));
            true
        }
    }

    fn system_seed_or(default: u64) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| u64::try_from(d.as_nanos()).ok())
            .unwrap_or(default)
    }

    /// Initialize the RNG with entropy from system time.
    ///
    /// This is the canonical lazy initializer used by `ensure_seed_initialized()`
    /// when the seed has not been explicitly initialized with `init_seed_with_value()`.
    /// It uses system time to generate entropy, with a fallback counter if needed.
    ///
    /// Multiple threads may race through `ensure_seed_initialized()` and call this
    /// function concurrently. In that case, the last writer wins and the seed is
    /// non-deterministic. This is acceptable for the non-cryptographic use cases
    /// this module serves.
    ///
    /// Note: This function uses Mutex for atomic initialization.
    /// For explicit seed control, use `init_seed_with_value()` instead.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned by another thread.
    pub fn init_seed() {
        let fallback = FALLBACK_COUNTER.fetch_add(1, Ordering::Relaxed);
        let seed = system_seed_or(fallback);
        let mut guard = SEED.lock().unwrap();
        if guard.is_none() {
            *guard = Some(AtomicU64::new(seed));
        }
    }

    /// Ensure the RNG seed is initialized before use.
    ///
    /// This performs lazy initialization by calling `init_seed()` if the seed hasn't
    /// been explicitly initialized with `init_seed_with_value()`.
    fn ensure_seed_initialized() {
        // Check if already initialized (fast path without lock)
        {
            let guard = SEED.lock().unwrap();
            if guard.is_some() {
                return;
            }
        }
        // Need to initialize - acquire lock and double-check
        let fallback = FALLBACK_COUNTER.fetch_add(1, Ordering::Relaxed);
        let seed = system_seed_or(fallback);
        let mut guard = SEED.lock().unwrap();
        if guard.is_none() {
            *guard = Some(AtomicU64::new(seed));
        }
    }

    /// Generate a pseudo-random u64.
    ///
    /// Returns a random u64 value using a linear congruential generator (LCG).
    /// The seed is lazily initialized on first call if not previously set via
    /// `init_seed_with_value()`.
    ///
    /// # Thread Safety
    ///
    /// This function uses atomic operations to ensure thread-safe access to the seed.
    /// The read-modify-write operation is performed atomically using `fetch_update`.
    ///
    /// # Panics
    ///
    /// This function will panic if the internal `fetch_update` closure fails to return
    /// `Some`, which should never occur in practice as the closure always returns `Some`.
    ///
    /// # Examples
    ///
    /// ```
    /// use canopus_core::utilities::simple_rng::next_u64;
    ///
    /// let val = next_u64();
    /// assert!(val < u64::MAX);
    /// ```
    #[must_use]
    pub fn next_u64() -> u64 {
        ensure_seed_initialized();

        // Perform atomic fetch_update while holding the lock.
        // The lock is held only for the duration of the atomic operation,
        // which is very fast. This avoids the complexity of taking ownership
        // and risking poisoning.
        let prev = {
            let guard = SEED.lock().unwrap();
            guard
                .as_ref()
                .expect("SEED must be initialized")
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |curr| {
                    Some(curr.wrapping_mul(1_103_515_245).wrapping_add(12_345))
                })
        }
        .expect("closure always returns Some");

        // Recompute prev * A + C to return the newly stored value, not the stale
        // pre-update value. This avoids returning a value that was the seed itself.
        prev.wrapping_mul(1_103_515_245).wrapping_add(12_345)
    }

    /// Generate a pseudo-random u32.
    ///
    /// Returns a random u32 value derived from `next_u64()`.
    #[must_use]
    pub fn next_u32() -> u32 {
        #[allow(clippy::cast_possible_truncation)]
        {
            next_u64() as u32
        }
    }

    /// Generate a pseudo-random f64 in the range [0.0, 1.0).
    ///
    /// Returns a random floating-point value uniformly distributed in the
    /// half-open interval [0.0, 1.0).
    #[must_use]
    pub fn next_f64() -> f64 {
        #[allow(clippy::cast_precision_loss)]
        {
            (next_u64() as f64) / (u64::MAX as f64 + 1.0)
        }
    }

    /// Reset the seed for testing purposes.
    ///
    /// This allows tests to verify that `ensure_seed_initialized()` properly
    /// initializes the seed when called from functions like `next_u64()`.
    ///
    /// # Preconditions
    ///
    /// This should only be called in tests with proper synchronization (e.g.,
    /// under a test lock) to prevent race conditions.
    #[cfg(test)]
    pub fn reset_initialized_for_tests() {
        let mut guard = SEED.lock().unwrap();
        *guard = None;
        FALLBACK_COUNTER.store(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::simple_rng::{
        init_seed, init_seed_with_value, next_f64, next_u32, next_u64, reset_initialized_for_tests,
    };
    use super::*;
    use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
    use std::thread;

    // Global lock to prevent parallel test execution from corrupting shared RNG state
    static TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn test_lock() -> MutexGuard<'static, ()> {
        let lock = TEST_LOCK.lock().expect("RNG test lock poisoned");
        reset_initialized_for_tests();
        lock
    }

    #[test]
    fn test_validate_config_data_empty() {
        let result = validate_config_data("");
        assert!(result.is_err());
        if let Err(crate::CoreError::ValidationError(msg)) = result {
            assert!(msg.contains("empty"));
        }
    }

    #[test]
    fn test_validate_config_data_valid_json() {
        let result = validate_config_data(r#"{"key": "value"}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_config_data_invalid_json() {
        let result = validate_config_data("{invalid json");
        assert!(result.is_err());
        if let Err(crate::CoreError::ValidationError(msg)) = result {
            assert!(msg.contains("Invalid JSON"));
        }
    }

    #[test]
    fn test_init_seed_with_value() {
        let _lock = test_lock();

        init_seed_with_value(12345);
        let val1 = next_u64();
        let val2 = next_u64();

        // Reset to same seed
        reset_initialized_for_tests();
        init_seed_with_value(12345);
        let val1_repeat = next_u64();
        let val2_repeat = next_u64();

        // Same seed should produce same sequence
        assert_eq!(val1, val1_repeat);
        assert_eq!(val2, val2_repeat);
    }

    #[test]
    fn test_next_u64_must_use() {
        let _lock = test_lock();

        init_seed_with_value(100);
        let val1 = next_u64();
        let val2 = next_u64();
        // Consecutive calls should return different values
        assert_ne!(
            val1, val2,
            "Consecutive calls should return different values"
        );
    }

    #[test]
    fn test_next_u64_consistency() {
        let _lock = test_lock();

        init_seed_with_value(42);

        let sequence: Vec<u64> = (0..5).map(|_| next_u64()).collect();

        // Reset to same seed and verify sequence is reproducible
        reset_initialized_for_tests();
        init_seed_with_value(42);
        for (i, expected) in sequence.iter().enumerate() {
            let actual = next_u64();
            assert_eq!(
                actual, *expected,
                "Sequence position {i} does not match after reset"
            );
        }
    }

    #[test]
    fn test_next_u32_in_range() {
        let _lock = test_lock();

        init_seed();
        let _val = next_u32();
    }

    #[test]
    fn test_next_f64_in_range() {
        let _lock = test_lock();

        init_seed();
        let val = next_f64();
        assert!(
            (0.0..1.0).contains(&val),
            "next_f64() should return value in [0.0, 1.0), got {val}"
        );
    }

    #[test]
    fn test_lazy_initialization() {
        let _lock = test_lock();

        // Reset to uninitialized state by clearing the INITIALIZED flag
        // This ensures ensure_seed_initialized() is actually exercised
        reset_initialized_for_tests();

        // First call should auto-initialize via ensure_seed_initialized()
        // and NOT return 0 (which would indicate uninitialized state)
        let val1 = next_u64();
        assert_ne!(
            val1, 0,
            "Should auto-initialize and return first generated value"
        );

        // Second call should produce different value
        let val2 = next_u64();
        assert_ne!(val1, val2, "Should produce different values");
    }

    #[test]
    fn test_thread_safety() {
        let _lock = test_lock();

        init_seed_with_value(999);

        let results = Arc::new(Mutex::new(Vec::new()));
        let mut handles = vec![];

        for _ in 0..10 {
            let results_clone = Arc::clone(&results);
            let handle = thread::spawn(move || {
                for _ in 0..100 {
                    let val = next_u64();
                    let mut results = results_clone.lock().unwrap();
                    results.push(val);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        {
            let results = results.lock().unwrap();
            // With 10 threads each generating 100 values, we should have 1000 values
            let len = results.len();
            assert_eq!(len, 1000);

            // Check that all values are unique (very high probability for 1000 random u64s)
            let unique_values: std::collections::HashSet<u64> = results.iter().copied().collect();
            drop(results);
            assert_eq!(
                unique_values.len(),
                len,
                "All values should be unique (no race conditions)"
            );
        }
    }

    #[test]
    fn test_fetch_update_atomicity() {
        let _lock = test_lock();

        // Test that fetch_update produces correct sequence
        init_seed_with_value(1);
        let seq1: Vec<u64> = (0..10).map(|_| next_u64()).collect();

        reset_initialized_for_tests();
        init_seed_with_value(1);
        let seq2: Vec<u64> = (0..10).map(|_| next_u64()).collect();

        // Sequences should be identical
        assert_eq!(
            seq1, seq2,
            "Sequences should match when starting from same seed"
        );

        // Each value in sequence should be different from the previous
        for window in seq1.windows(2) {
            assert_ne!(
                window[0], window[1],
                "Consecutive values should differ: {} vs {}",
                window[0], window[1]
            );
        }
    }

    #[test]
    fn test_seed_zero_deterministic() {
        let _lock = test_lock();

        // Seed 0 is a valid deterministic seed, not an uninitialized sentinel
        init_seed_with_value(0);
        let seq1: Vec<u64> = (0..10).map(|_| next_u64()).collect();

        // Reset and verify same sequence
        reset_initialized_for_tests();
        init_seed_with_value(0);
        let seq2: Vec<u64> = (0..10).map(|_| next_u64()).collect();

        assert_eq!(seq1, seq2, "Seed 0 should produce deterministic sequence");
    }

    #[test]
    fn test_seed_zero_with_concurrent_access() {
        let _lock = test_lock();

        // Verify seed 0 produces deterministic sequence even with concurrent next_u64 calls
        // This tests the fix for the race condition where seed 0 could be overwritten
        init_seed_with_value(0);

        let results = Arc::new(Mutex::new(Vec::new()));
        let mut handles = vec![];

        for _ in 0..5 {
            let results_clone = Arc::clone(&results);
            let handle = thread::spawn(move || {
                for _ in 0..20 {
                    let val = next_u64();
                    let mut results = results_clone.lock().unwrap();
                    results.push(val);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Now reset to seed 0 and verify we get the same sequence
        reset_initialized_for_tests();
        init_seed_with_value(0);

        let results2 = Arc::new(Mutex::new(Vec::new()));
        let mut handles2 = vec![];

        for _ in 0..5 {
            let results_clone = Arc::clone(&results2);
            let handle = thread::spawn(move || {
                for _ in 0..20 {
                    let val = next_u64();
                    let mut results = results_clone.lock().unwrap();
                    results.push(val);
                }
            });
            handles2.push(handle);
        }

        for handle in handles2 {
            handle.join().unwrap();
        }

        // Compare sorted results (order may differ due to thread scheduling)
        let mut seq1 = results.lock().unwrap().clone();
        let mut seq2 = results2.lock().unwrap().clone();
        seq1.sort_unstable();
        seq2.sort_unstable();

        assert_eq!(
            seq1, seq2,
            "Seed 0 should produce same multiset of values regardless of thread interleaving"
        );
    }

    #[test]
    fn test_init_seed_with_value_returns_false_on_second_call() {
        let _lock = test_lock();

        // First call should succeed
        assert!(init_seed_with_value(42), "First init should succeed");
        let seq_42: Vec<u64> = (0..5).map(|_| next_u64()).collect();

        // Second call with different seed should be rejected
        assert!(!init_seed_with_value(99), "Second init should be rejected");

        // Verify sequence still matches seed 42
        reset_initialized_for_tests();
        assert!(init_seed_with_value(42));
        let seq_verify: Vec<u64> = (0..5).map(|_| next_u64()).collect();
        assert_eq!(seq_42, seq_verify, "Sequence should match original seed 42");
    }

    #[test]
    fn test_next_f64_multiple_samples_in_range() {
        let _lock = test_lock();

        init_seed_with_value(12345);
        for i in 0..100 {
            let val = next_f64();
            assert!(
                (0.0..1.0).contains(&val),
                "Sample {i}: next_f64() returned {val}, expected [0.0, 1.0)"
            );
        }
    }
}
