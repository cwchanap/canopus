//! Unified inbox system for AI agent notifications.
//!
//! This crate provides a centralized inbox for collecting and managing
//! notifications from various AI coding agents (Claude Code, Codex, Windsurf, `OpenCode`).
//!
//! # Features
//!
//! - SQLite-backed persistent storage
//! - Desktop notifications (macOS/Linux)
//! - Query/filter support
//! - Automatic 7-day cleanup
//!
//! # Example
//!
//! ```ignore
//! use canopus_inbox::{SqliteStore, InboxStore, NewInboxItem, SourceAgent};
//!
//! #[tokio::main]
//! async fn main() -> canopus_inbox::Result<()> {
//!     let store = SqliteStore::open_default()?;
//!
//!     // Add a new inbox item
//!     let item = store.insert(NewInboxItem::new(
//!         "my-project",
//!         "Tests passing",
//!         "Review PR #123",
//!         SourceAgent::ClaudeCode,
//!     )).await?;
//!
//!     // Send desktop notification
//!     canopus_inbox::notify::send_notification(&item)?;
//!
//!     Ok(())
//! }
//! ```

#![allow(clippy::module_name_repetitions)]

pub mod error;
pub mod item;
pub mod notify;
pub mod store;

// Re-export main types for convenience
pub use error::{InboxError, Result};
pub use item::{InboxFilter, InboxItem, InboxStatus, NewInboxItem, SourceAgent};
pub use notify::{
    format_item_notification, format_summary_notification, truncate, NotificationBackend,
};
pub use store::escape_like_pattern;
pub use store::{InboxStore, SqliteStore};

/// Default retention period in days.
pub const DEFAULT_RETENTION_DAYS: i64 = 7;
