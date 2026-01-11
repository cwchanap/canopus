//! Inbox storage layer with `SQLite` implementation.

#![allow(clippy::significant_drop_tightening)] // Lock guards held across DB operations

use crate::error::{InboxError, Result};
use crate::item::{InboxFilter, InboxItem, InboxStatus, NewInboxItem, SourceAgent};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};

/// SQL schema for the inbox database.
const SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS inbox_items (
    id TEXT PRIMARY KEY,
    project_name TEXT NOT NULL,
    status_summary TEXT NOT NULL,
    action_required TEXT NOT NULL,
    source_agent TEXT NOT NULL,
    details TEXT,
    status TEXT NOT NULL DEFAULT 'unread',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    dismissed_at INTEGER,
    notified INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_inbox_status ON inbox_items(status);
CREATE INDEX IF NOT EXISTS idx_inbox_created_at ON inbox_items(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_inbox_source_agent ON inbox_items(source_agent);
CREATE INDEX IF NOT EXISTS idx_inbox_project ON inbox_items(project_name);
CREATE INDEX IF NOT EXISTS idx_inbox_active ON inbox_items(status, created_at DESC)
    WHERE status != 'dismissed';
";

/// Trait for inbox storage operations.
#[async_trait]
pub trait InboxStore: Send + Sync {
    /// Insert a new inbox item.
    async fn insert(&self, item: NewInboxItem) -> Result<InboxItem>;

    /// List items matching the given filter.
    async fn list(&self, filter: InboxFilter) -> Result<Vec<InboxItem>>;

    /// Get a single item by ID.
    async fn get(&self, id: &str) -> Result<Option<InboxItem>>;

    /// Mark an item as read.
    async fn mark_read(&self, id: &str) -> Result<()>;

    /// Dismiss an item.
    async fn dismiss(&self, id: &str) -> Result<()>;

    /// Mark an item as notified.
    async fn mark_notified(&self, id: &str) -> Result<()>;

    /// Delete items older than the given number of days.
    async fn cleanup_older_than(&self, days: i64) -> Result<usize>;

    /// Count items matching the given filter.
    async fn count(&self, filter: InboxFilter) -> Result<usize>;
}

/// SQLite-backed inbox storage.
#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for SqliteStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteStore").finish()
    }
}

impl SqliteStore {
    /// Opens or creates the inbox database at the default location.
    ///
    /// Default location: `$HOME/.canopus/inbox.db`
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `HOME` environment variable is not set
    /// - The database directory cannot be created
    /// - The database cannot be opened or initialized
    pub fn open_default() -> Result<Self> {
        let base = std::env::var("HOME").map(PathBuf::from).map_err(|_| {
            InboxError::InvalidInput(
                "HOME environment variable not set; use explicit database path".into(),
            )
        })?;
        let dir = base.join(".canopus");
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("inbox.db");
        Self::open(&db_path)
    }

    /// Opens or creates the inbox database at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or initialized.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrency
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| InboxError::Database(format!("Failed to set WAL mode: {e}")))?;

        // Initialize schema
        conn.execute_batch(SCHEMA)?;

        info!("Inbox database opened at {:?}", path);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Opens an in-memory database for testing.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be initialized.
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[async_trait]
impl InboxStore for SqliteStore {
    async fn insert(&self, new_item: NewInboxItem) -> Result<InboxItem> {
        let item = InboxItem::from_new(new_item);
        let item_clone = item.clone();

        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();

            let details_json = item_clone
                .details
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;

            conn.execute(
                "INSERT INTO inbox_items
                 (id, project_name, status_summary, action_required, source_agent,
                   details, status, created_at, updated_at, dismissed_at, notified)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    item_clone.id,
                    item_clone.project_name,
                    item_clone.status_summary,
                    item_clone.action_required,
                    item_clone.source_agent.as_str(),
                    details_json,
                    item_clone.status.as_str(),
                    item_clone.created_at.timestamp(),
                    item_clone.updated_at.timestamp(),
                    item_clone.dismissed_at.map(|dt| dt.timestamp()),
                    i32::from(item_clone.notified),
                ],
            )?;

            debug!("Inserted inbox item: {}", item_clone.id);
            Ok::<InboxItem, InboxError>(item_clone)
        })
        .await
        .map_err(|e| InboxError::Database(format!("Task join error: {e}")))?
    }

    async fn list(&self, filter: InboxFilter) -> Result<Vec<InboxItem>> {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();

            let mut sql = String::from(
                "SELECT id, project_name, status_summary, action_required, source_agent,
                        details, status, created_at, updated_at, dismissed_at, notified
                 FROM inbox_items WHERE 1=1",
            );
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(status) = &filter.status {
                sql.push_str(" AND status = ?");
                params_vec.push(Box::new(status.as_str().to_string()));
            }

            if let Some(agent) = &filter.source_agent {
                sql.push_str(" AND source_agent = ?");
                params_vec.push(Box::new(agent.as_str().to_string()));
            }

            if let Some(project) = &filter.project {
                sql.push_str(" AND project_name LIKE ? ESCAPE '\\'");
                let escaped = escape_like_pattern(project);
                params_vec.push(Box::new(format!("%{escaped}%")));
            }

            sql.push_str(" ORDER BY created_at DESC");

            if let Some(limit) = filter.limit {
                sql.push_str(" LIMIT ?");
                params_vec.push(Box::new(limit));
            }

            let mut stmt = conn.prepare(&sql)?;
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(AsRef::as_ref).collect();

            let items = stmt
                .query_map(params_refs.as_slice(), row_to_inbox_item)?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            Ok(items)
        })
        .await
        .map_err(|e| InboxError::Database(format!("Task join error: {e}")))?
    }

    async fn get(&self, id: &str) -> Result<Option<InboxItem>> {
        let id = id.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT id, project_name, status_summary, action_required, source_agent,
                        details, status, created_at, updated_at, dismissed_at, notified
                 FROM inbox_items WHERE id = ?1",
            )?;

            let item = stmt.query_row(params![id], row_to_inbox_item).optional()?;

            Ok(item)
        })
        .await
        .map_err(|e| InboxError::Database(format!("Task join error: {e}")))?
    }

    async fn mark_read(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let now = Utc::now().timestamp();

            let updated = conn.execute(
                "UPDATE inbox_items SET status = 'read', updated_at = ?1
                 WHERE id = ?2 AND status = 'unread'",
                params![now, id],
            )?;

            if updated == 0 {
                debug!("Item {} not found or already read", id);
            } else {
                debug!("Marked item {} as read", id);
            }

            Ok(())
        })
        .await
        .map_err(|e| InboxError::Database(format!("Task join error: {e}")))?
    }

    async fn dismiss(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let now = Utc::now().timestamp();

            let updated = conn.execute(
                "UPDATE inbox_items SET status = 'dismissed', updated_at = ?1, dismissed_at = ?1
                 WHERE id = ?2 AND status != 'dismissed'",
                params![now, id],
            )?;

            if updated == 0 {
                return Err(InboxError::NotFound(format!(
                    "Item {id} not found or already dismissed"
                )));
            }

            debug!("Dismissed item {}", id);
            Ok(())
        })
        .await
        .map_err(|e| InboxError::Database(format!("Task join error: {e}")))?
    }

    async fn mark_notified(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let now = Utc::now().timestamp();

            conn.execute(
                "UPDATE inbox_items SET notified = 1, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;

            debug!("Marked item {} as notified", id);
            Ok(())
        })
        .await
        .map_err(|e| InboxError::Database(format!("Task join error: {e}")))?
    }

    async fn cleanup_older_than(&self, days: i64) -> Result<usize> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let cutoff = Utc::now()
                .checked_sub_signed(chrono::Duration::days(days))
                .ok_or_else(|| InboxError::InvalidInput("Invalid duration".into()))?
                .timestamp();

            let deleted = conn.execute(
                "DELETE FROM inbox_items WHERE created_at < ?1",
                params![cutoff],
            )?;

            if deleted > 0 {
                info!("Cleaned up {} old inbox items", deleted);
            }

            Ok(deleted)
        })
        .await
        .map_err(|e| InboxError::Database(format!("Task join error: {e}")))?
    }

    async fn count(&self, filter: InboxFilter) -> Result<usize> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();

            let mut sql = String::from("SELECT COUNT(*) FROM inbox_items WHERE 1=1");
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(status) = &filter.status {
                sql.push_str(" AND status = ?");
                params_vec.push(Box::new(status.as_str().to_string()));
            }

            if let Some(agent) = &filter.source_agent {
                sql.push_str(" AND source_agent = ?");
                params_vec.push(Box::new(agent.as_str().to_string()));
            }

            if let Some(project) = &filter.project {
                sql.push_str(" AND project_name LIKE ? ESCAPE '\\'");
                let escaped = escape_like_pattern(project);
                params_vec.push(Box::new(format!("%{escaped}%")));
            }

            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(AsRef::as_ref).collect();

            let count: i64 = conn.query_row(&sql, params_refs.as_slice(), |row| row.get(0))?;

            // COUNT(*) is always non-negative, safe to convert
            Ok(usize::try_from(count).unwrap_or(0))
        })
        .await
        .map_err(|e| InboxError::Database(format!("Task join error: {e}")))?
    }
}

/// Escapes SQL LIKE pattern special characters (%, _, \).
pub fn escape_like_pattern(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '%' | '_' | '\\' => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
    }
    result
}

/// Helper function to convert a database row to an `InboxItem`.
fn row_to_inbox_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<InboxItem> {
    let details_str: Option<String> = row.get(5)?;
    let details = details_str.and_then(|s| serde_json::from_str(&s).ok());

    let created_ts: i64 = row.get(7)?;
    let updated_ts: i64 = row.get(8)?;
    let dismissed_ts: Option<i64> = row.get(9)?;

    Ok(InboxItem {
        id: row.get(0)?,
        project_name: row.get(1)?,
        status_summary: row.get(2)?,
        action_required: row.get(3)?,
        source_agent: SourceAgent::from_str(&row.get::<_, String>(4)?),
        details,
        status: InboxStatus::from_str(&row.get::<_, String>(6)?),
        created_at: DateTime::from_timestamp(created_ts, 0).unwrap_or_else(Utc::now),
        updated_at: DateTime::from_timestamp(updated_ts, 0).unwrap_or_else(Utc::now),
        dismissed_at: dismissed_ts.and_then(|ts| DateTime::from_timestamp(ts, 0)),
        notified: row.get::<_, i32>(10)? != 0,
    })
}
