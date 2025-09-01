//! Port allocation and reservation system
//!
//! This module provides a deterministic, race-safe port allocator that can:
//! - Try a preferred port first
//! - Fall back to a deterministic sequence of ports
//! - Maintain in-process reservations to avoid conflicts
//! - Use actual TCP binding to probe port availability

use crate::{CoreError, Result};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::{
    net::{SocketAddr, TcpListener},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{debug, warn};

/// Port range for automatic allocation - starting port
pub const DEFAULT_PORT_RANGE_START: u16 = 30_000;
/// Port range for automatic allocation - ending port
pub const DEFAULT_PORT_RANGE_END: u16 = 60_000;
/// Maximum number of ports to try before giving up
pub const MAX_ALLOCATION_ATTEMPTS: usize = 1000;

/// Metadata about a port reservation
#[derive(Debug, Clone, Copy)]
pub struct ReservationMeta {
    /// Process ID that made the reservation
    pub pid: u32,
    /// Thread ID that made the reservation
    pub thread_id: u64,
    /// Timestamp when the reservation was made
    pub timestamp: u64,
}

/// Global reservation table to track allocated ports in-process
static RESERVATIONS: Lazy<DashMap<u16, ReservationMeta>> = Lazy::new(DashMap::new);

/// Counter for generating deterministic thread-specific sequences
static THREAD_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A guard that holds a port reservation and the underlying TCP listener
/// 
/// The port is automatically released when this guard is dropped.
#[derive(Debug)]
pub struct PortGuard {
    port: u16,
    _listener: TcpListener,
}

impl PortGuard {
    /// Get the allocated port number
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the socket address this port is bound to
    pub fn addr(&self) -> Result<SocketAddr> {
        self._listener.local_addr().map_err(CoreError::from)
    }
}

impl Drop for PortGuard {
    fn drop(&mut self) {
        release_port(self.port);
        debug!("Released port {} on drop", self.port);
    }
}

/// Port allocator providing deterministic, race-safe port allocation
#[derive(Debug, Default, Clone, Copy)]
pub struct PortAllocator {
    range_start: u16,
    range_end: u16,
}

impl PortAllocator {
    /// Create a new port allocator with default port range
    pub fn new() -> Self {
        Self::with_range(DEFAULT_PORT_RANGE_START, DEFAULT_PORT_RANGE_END)
    }

    /// Create a new port allocator with custom port range
    pub fn with_range(start: u16, end: u16) -> Self {
        Self {
            range_start: start,
            range_end: end,
        }
    }

    /// Reserve a port, optionally trying a preferred port first
    /// 
    /// If a preferred port is provided and available, it will be used.
    /// Otherwise, falls back to a deterministic sequence of ports within the configured range.
    pub fn reserve(&self, preferred: Option<u16>) -> Result<PortGuard> {
        let mut attempts = 0;

        // Try preferred port first if provided
        if let Some(port) = preferred {
            attempts += 1;
            match self.try_reserve_port_internal(port) {
                Ok(guard) => {
                    debug!("Successfully reserved preferred port {}", port);
                    return Ok(guard);
                }
                Err(CoreError::PortInUse(_)) => {
                    debug!("Preferred port {} is already in use, falling back to sequence", port);
                }
                Err(e) => return Err(e),
            }
        }

        // Fall back to deterministic sequence
        let sequence = self.generate_port_sequence();
        for port in sequence {
            if attempts >= MAX_ALLOCATION_ATTEMPTS {
                break;
            }
            attempts += 1;

            match self.try_reserve_port_internal(port) {
                Ok(guard) => {
                    debug!("Successfully reserved port {} after {} attempts", port, attempts);
                    return Ok(guard);
                }
                Err(CoreError::PortInUse(_)) => {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        Err(CoreError::NoAvailablePort { tried: attempts })
    }

    /// Try to reserve a specific port
    #[cfg(test)]
    pub fn try_reserve_port(&self, port: u16) -> Result<PortGuard> {
        self.try_reserve_port_internal(port)
    }
    
    /// Try to reserve a specific port (internal implementation)
    fn try_reserve_port_internal(&self, port: u16) -> Result<PortGuard> {
        // Check if already reserved in-process
        if RESERVATIONS.contains_key(&port) {
            return Err(CoreError::PortInUse(port));
        }

        // Try to bind to the port to check OS availability
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = TcpListener::bind(addr).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                CoreError::PortInUse(port)
            } else {
                CoreError::from(e)
            }
        })?;

        // Reserve the port in our tracking table
        let meta = ReservationMeta {
            pid: process::id(),
            thread_id: get_thread_id(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        // Double-check reservation to handle race conditions
        if let Some(_existing) = RESERVATIONS.insert(port, meta) {
            // Someone else reserved it between our check and insert
            warn!("Race condition detected for port {}, releasing", port);
            return Err(CoreError::PortInUse(port));
        }

        Ok(PortGuard {
            port,
            _listener: listener,
        })
    }

    /// Generate a deterministic sequence of ports to try
    fn generate_port_sequence(&self) -> impl Iterator<Item = u16> + '_ {
        let seed = self.calculate_deterministic_seed();
        let range_size = self.range_end - self.range_start;
        let start_offset = (seed % u64::from(range_size)) as u16;
        
        (0..range_size)
            .map(move |i| {
                let offset = (start_offset + i) % range_size;
                self.range_start + offset
            })
    }

    /// Calculate a deterministic seed based on process and thread information
    fn calculate_deterministic_seed(&self) -> u64 {
        let pid = u64::from(process::id());
        let thread_id = get_thread_id();
        
        // Use a simple hash combining process ID and thread ID
        // This ensures different processes and threads get different sequences
        // but the same process/thread combination always gets the same sequence
        pid.wrapping_mul(31).wrapping_add(thread_id)
    }
}

/// Get a unique identifier for the current thread
fn get_thread_id() -> u64 {
    thread_local! {
        static THREAD_ID: u64 = THREAD_COUNTER.fetch_add(1, Ordering::Relaxed);
    }
    THREAD_ID.with(|&id| id)
}

/// Explicitly release a reserved port
/// 
/// This is called automatically when a PortGuard is dropped, but can be called manually if needed.
pub fn release_port(port: u16) {
    if let Some(_meta) = RESERVATIONS.remove(&port) {
        debug!("Explicitly released port {}", port);
    }
}

/// Get information about currently reserved ports
/// 
/// This is primarily useful for debugging and testing.
#[cfg(test)]
pub fn get_reservations() -> Vec<(u16, ReservationMeta)> {
    RESERVATIONS.iter().map(|entry| (*entry.key(), *entry.value())).collect()
}

/// Clear all reservations
/// 
/// This is primarily useful for testing to ensure clean state between tests.
#[cfg(test)]
pub fn clear_reservations() {
    RESERVATIONS.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_allocator_creation() {
        let allocator = PortAllocator::new();
        assert_eq!(allocator.range_start, DEFAULT_PORT_RANGE_START);
        assert_eq!(allocator.range_end, DEFAULT_PORT_RANGE_END);

        let allocator = PortAllocator::with_range(8000, 9000);
        assert_eq!(allocator.range_start, 8000);
        assert_eq!(allocator.range_end, 9000);
    }

    #[test]
    fn test_preferred_port_success() {
        clear_reservations();
        let allocator = PortAllocator::new();
        
        // Try to reserve a port in a high range that's likely to be free
        let preferred_port = 45123;
        let guard = allocator.reserve(Some(preferred_port));
        
        match guard {
            Ok(g) => {
                assert_eq!(g.port(), preferred_port);
                // Verify it's in our reservation table
                assert!(RESERVATIONS.contains_key(&preferred_port));
            }
            Err(CoreError::PortInUse(_)) => {
                // Port was already in use by the system, which is acceptable in tests
                println!("Port {} was already in use by the system", preferred_port);
            }
            Err(e) => panic!("Unexpected error: {}", e),
        }
    }

    #[test]
    fn test_port_collision() {
        clear_reservations();
        let allocator = PortAllocator::new();
        
        // Try to reserve a port in a high range that's likely to be free
        let preferred_port = 45124;
        let guard1 = allocator.reserve(Some(preferred_port));
        
        match guard1 {
            Ok(_guard1) => {
                // Verify it's properly reserved in our table
                assert!(RESERVATIONS.contains_key(&preferred_port));
                
                // Try to directly reserve the same port using try_reserve_port
                // This tests our internal collision detection
                let collision_result = allocator.try_reserve_port(preferred_port);
                match collision_result {
                    Err(CoreError::PortInUse(port)) => {
                        assert_eq!(port, preferred_port);
                    }
                    Ok(_guard) => {
                        panic!("Expected port collision error for direct reservation");
                    }
                    Err(e) => {
                        panic!("Unexpected error type: {}", e);
                    }
                }
                
                // Also test that regular reserve() with the same preferred port 
                // falls back correctly (should succeed with different port)
                let guard2 = allocator.reserve(Some(preferred_port));
                match guard2 {
                    Ok(guard) => {
                        // Should get a different port from fallback sequence
                        assert_ne!(guard.port(), preferred_port);
                    }
                    Err(_e) => {
                        // This could happen if all ports in range are busy, which is ok
                    }
                }
                // _guard1 is dropped here, releasing the port
            }
            Err(CoreError::PortInUse(_)) => {
                // Port was already in use by the system - skip the test
                println!("Port {} was already in use by the system, skipping collision test", preferred_port);
            }
            Err(e) => panic!("Unexpected error during first reservation: {}", e),
        }
    }

    #[test]
    fn test_port_release_on_drop() {
        clear_reservations();
        let allocator = PortAllocator::new();
        
        let preferred_port = 45125;
        let reservations_before = get_reservations().len();
        
        {
            let _guard = allocator.reserve(Some(preferred_port));
            // Port should be reserved
            if _guard.is_ok() {
                assert!(RESERVATIONS.contains_key(&preferred_port));
            }
        } // guard drops here
        
        // Port should be released
        assert!(!RESERVATIONS.contains_key(&preferred_port));
        let reservations_after = get_reservations().len();
        assert_eq!(reservations_before, reservations_after);
    }

    #[test]
    fn test_deterministic_sequence() {
        let allocator = PortAllocator::with_range(45000, 45010);
        
        // Generate sequence twice and verify they're identical
        let seq1: Vec<u16> = allocator.generate_port_sequence().take(10).collect();
        let seq2: Vec<u16> = allocator.generate_port_sequence().take(10).collect();
        
        assert_eq!(seq1, seq2);
        assert!(!seq1.is_empty());
        
        // All ports should be within range
        for port in seq1 {
            assert!((45000..45010).contains(&port));
        }
    }

    #[test]
    fn test_fallback_to_sequence() {
        clear_reservations();
        let allocator = PortAllocator::with_range(45200, 45210);
        
        // Reserve without preferred port, should use sequence
        let guard = allocator.reserve(None);
        
        match guard {
            Ok(g) => {
                let port = g.port();
                assert!((45200..45210).contains(&port));
                assert!(RESERVATIONS.contains_key(&port));
            }
            Err(CoreError::NoAvailablePort { tried }) => {
                // All ports in the small range were unavailable
                assert!(tried > 0);
            }
            Err(e) => panic!("Unexpected error: {}", e),
        }
    }

    #[test]
    fn test_explicit_release() {
        clear_reservations();
        let allocator = PortAllocator::new();
        let preferred_port = 45126;
        
        if let Ok(guard) = allocator.reserve(Some(preferred_port)) {
            let port = guard.port();
            assert!(RESERVATIONS.contains_key(&port));
            
            // Explicitly release (this will be called again on drop, which should be fine)
            release_port(port);
            assert!(!RESERVATIONS.contains_key(&port));
        }
    }

    #[test]
    fn test_get_addr() {
        clear_reservations();
        let allocator = PortAllocator::new();
        let preferred_port = 45127;
        
        if let Ok(guard) = allocator.reserve(Some(preferred_port)) {
            let addr = guard.addr().expect("Should be able to get address");
            assert_eq!(addr.port(), preferred_port);
            assert!(addr.is_ipv4());
        }
    }
}
