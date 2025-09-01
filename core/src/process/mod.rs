//! Process management utilities for the Canopus core library
//!
//! This module provides cross-platform process management capabilities,
//! with platform-specific implementations for safe process spawning,
//! lifecycle management, and cleanup.
//!
//! ## Platform Support
//!
//! - **Unix**: Full support with process groups for safe cleanup
//! - **Windows**: Coming in T2.2 (Job Object-based lifecycle)
//!
//! ## Safety
//!
//! The implementations prioritize safe process management by:
//! - Using process groups (Unix) or job objects (Windows) for reliable cleanup
//! - Providing both graceful and forceful termination methods
//! - Preventing orphaned processes through proper lifecycle management

#[cfg(unix)]
pub mod unix;

#[cfg(unix)]
pub use unix::*;
