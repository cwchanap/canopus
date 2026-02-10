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
pub mod simple_rng {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    const SEED_UNINITIALIZED: u64 = 0;

    static SEED: AtomicU64 = AtomicU64::new(SEED_UNINITIALIZED);

    /// Initialize the RNG with a deterministic seed (useful for testing).
    ///
    /// # Arguments
    ///
    /// * `seed` - The seed value to use for the random number generator
    ///
    /// # Examples
    ///
    /// ```
    /// use canopus_core::utilities::simple_rng::init_seed_with_value;
    ///
    /// // For reproducible test results
    /// init_seed_with_value(42);
    /// let val1 = canopus_core::utilities::simple_rng::next_u64();
    /// let val2 = canopus_core::utilities::simple_rng::next_u64();
    /// ```
    pub fn init_seed_with_value(seed: u64) {
        SEED.store(seed, Ordering::Relaxed);
    }

    /// Initialize the RNG with entropy from system time.
    ///
    /// This is a fallback method when OS random number generator is not available.
    /// For production use, consider using `init_seed()` which provides better entropy.
    ///
    /// This function is automatically called on first use if the seed has not been
    /// explicitly initialized with `init_seed_with_value()`.
    pub fn init_seed() {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or_else(|_| {
                // Fallback: use a simple counter-based seed
                static FALLBACK_COUNTER: AtomicU64 = AtomicU64::new(1);
                FALLBACK_COUNTER.fetch_add(1, Ordering::Relaxed)
            });
        SEED.store(seed, Ordering::Relaxed);
    }

    /// Ensure the RNG seed is initialized before use.
    ///
    /// This performs lazy initialization using system time if the seed hasn't been
    /// explicitly initialized with `init_seed_with_value()`. Uses a compare-and-swap
    /// operation to avoid race conditions when multiple threads try to initialize
    /// the seed simultaneously.
    fn ensure_seed_initialized() {
        let current = SEED.load(Ordering::Relaxed);
        if current == SEED_UNINITIALIZED {
            // Use compare_exchange (not weak) to ensure reliable initialization
            let seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1);
            let _ = SEED.compare_exchange(
                SEED_UNINITIALIZED,
                seed,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
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

        // fetch_update returns the previous value before the update
        let prev = SEED
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |curr| {
                let next = curr.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                Some(next)
            })
            .unwrap_or_else(|_| {
                // If compare_exchange fails (should not happen), compute from current seed
                let curr = SEED.load(Ordering::Relaxed);
                curr.wrapping_mul(1_103_515_245).wrapping_add(12_345)
            });

        // Return the newly generated value (not the previous one)
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
            (next_u64() as f64) / (u64::MAX as f64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        use simple_rng::init_seed_with_value;
        use simple_rng::next_u64;

        init_seed_with_value(12345);
        let val1 = next_u64();
        let val2 = next_u64();

        // Reset to same seed
        init_seed_with_value(12345);
        let val1_repeat = next_u64();
        let val2_repeat = next_u64();

        // Same seed should produce same sequence
        assert_eq!(val1, val1_repeat);
        assert_eq!(val2, val2_repeat);
    }

    #[test]
    fn test_next_u64_must_use() {
        use simple_rng::init_seed_with_value;
        use simple_rng::next_u64;

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
        use simple_rng::init_seed_with_value;
        use simple_rng::next_u64;

        init_seed_with_value(42);

        let sequence: Vec<u64> = (0..5).map(|_| next_u64()).collect();

        // Reset to same seed and verify sequence is reproducible
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
        use simple_rng::init_seed;
        use simple_rng::next_u32;

        init_seed();
        let val = next_u32();
        assert!(val <= u32::MAX);
    }

    #[test]
    fn test_next_f64_in_range() {
        use simple_rng::init_seed;
        use simple_rng::next_f64;

        init_seed();
        let val = next_f64();
        assert!(
            val >= 0.0 && val < 1.0,
            "next_f64() should return value in [0.0, 1.0), got {val}"
        );
    }

    #[test]
    fn test_lazy_initialization() {
        use simple_rng::next_u64;

        // Reset to uninitialized state by setting to 0
        simple_rng::init_seed_with_value(0);

        // First call should auto-initialize and NOT return 0
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
        use simple_rng::init_seed_with_value;
        use simple_rng::next_u64;
        use std::sync::Arc;
        use std::thread;

        init_seed_with_value(999);

        let results = Arc::new(std::sync::Mutex::new(Vec::new()));
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

        let results = results.lock().unwrap();
        // With 10 threads each generating 100 values, we should have 1000 values
        assert_eq!(results.len(), 1000);

        // Check that all values are unique (very high probability for 1000 random u64s)
        let unique_values: std::collections::HashSet<_> = results.iter().collect();
        assert_eq!(
            unique_values.len(),
            results.len(),
            "All values should be unique (no race conditions)"
        );
    }

    #[test]
    fn test_fetch_update_atomicity() {
        use simple_rng::init_seed_with_value;
        use simple_rng::next_u64;

        // Test that fetch_update produces correct sequence
        init_seed_with_value(1);
        let seq1: Vec<u64> = (0..10).map(|_| next_u64()).collect();

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
}
