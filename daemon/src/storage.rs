//! `SQLite` storage for daemon runtime state
//!
//! Stores service state and PID persistently in $HOME/.canopus/canopus.db

use ipc::IpcError;
use ipc::Result as IpcResult;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// SQLite-backed storage adapter for daemon service metadata (port/hostname, etc.)
#[derive(Clone)]
#[allow(missing_debug_implementations)]
pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for SqliteStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteStorage").finish()
    }
}

#[async_trait::async_trait]
impl ipc::server::ServiceMetaStore for SqliteStorage {
    async fn set_port(&self, service_id: &str, port: Option<u16>) -> IpcResult<()> {
        self.update_port(service_id, port)
            .await
            .map_err(|e| IpcError::ProtocolError(e.to_string()))?;
        Ok(())
    }

    async fn set_hostname(&self, service_id: &str, hostname: Option<&str>) -> IpcResult<()> {
        self.update_hostname(service_id, hostname)
            .await
            .map_err(|e| IpcError::ProtocolError(e.to_string()))?;
        Ok(())
    }

    async fn get_port(&self, service_id: &str) -> IpcResult<Option<u16>> {
        self.fetch_port(service_id)
            .await
            .map_err(|e| IpcError::ProtocolError(e.to_string()))
    }

    async fn get_hostname(&self, service_id: &str) -> IpcResult<Option<String>> {
        self.fetch_hostname(service_id)
            .await
            .map_err(|e| IpcError::ProtocolError(e.to_string()))
    }

    async fn delete(&self, service_id: &str) -> IpcResult<()> {
        self.delete_service(service_id)
            .await
            .map_err(|e| IpcError::ProtocolError(e.to_string()))?;
        Ok(())
    }
}

impl SqliteStorage {
    /// Open or create the `SQLite` database at $HOME/.canopus/canopus.db.
    ///
    /// HOME must be set; the database path is resolved as `$HOME/.canopus/canopus.db`.
    ///
    /// # Errors
    /// Returns an error if HOME is not set, or if the database directory or schema cannot be
    /// created.
    pub fn open_default() -> anyhow::Result<Self> {
        let base = std::env::var("HOME").map(PathBuf::from).map_err(|_| {
            anyhow::anyhow!(
                "HOME must be set to open default database at $HOME/.canopus/canopus.db"
            )
        })?;
        let dir = base.join(".canopus");
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("canopus.db");

        let conn = Connection::open(db_path)?;
        // Enable WAL for better concurrency and durability
        if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
            warn!(
                "Failed to enable WAL journal mode: {}. Using default rollback journal.",
                e
            );
        }
        conn.execute_batch(
            r"
            CREATE TABLE IF NOT EXISTS services (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                state TEXT NOT NULL,
                pid INTEGER,
                port INTEGER,
                hostname TEXT,
                updated_at INTEGER NOT NULL
            );
            ",
        )?;

        // Best-effort migrations for existing DBs missing new columns
        match conn.execute("ALTER TABLE services ADD COLUMN port INTEGER", []) {
            Ok(_) => debug!("Added port column to services table"),
            Err(e) if e.to_string().contains("duplicate column") => {
                debug!("Port column already exists");
            }
            Err(e) => warn!("Migration failed for port column: {}", e),
        }
        match conn.execute("ALTER TABLE services ADD COLUMN hostname TEXT", []) {
            Ok(_) => debug!("Added hostname column to services table"),
            Err(e) if e.to_string().contains("duplicate column") => {
                debug!("Hostname column already exists");
            }
            Err(e) => warn!("Migration failed for hostname column: {}", e),
        }

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Delete a service row entirely
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn delete_service(&self, id: &str) -> anyhow::Result<()> {
        let id = id.to_string();
        tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || {
                let conn = conn.blocking_lock();
                conn.execute("DELETE FROM services WHERE id=?1", params![id])
                    .map(|_| ())
                    .map_err(|e| anyhow::anyhow!(e))
            }
        })
        .await??;
        Ok(())
    }

    /// Seed or upsert a service row
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn upsert_service(
        &self,
        id: &str,
        name: &str,
        state: &str,
        pid: Option<u32>,
        port: Option<u16>,
        hostname: Option<&str>,
    ) -> anyhow::Result<()> {
        let id = id.to_string();
        let name = name.to_string();
        let state = state.to_string();
        let hostname = hostname.map(ToString::to_string);
        tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || {
                let ts = now_ts();
                let conn = conn.blocking_lock();
                conn.execute(
                    r"
                    INSERT INTO services (id, name, state, pid, port, hostname, updated_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    ON CONFLICT(id) DO UPDATE SET
                        name=excluded.name,
                        state=excluded.state,
                        pid=excluded.pid,
                        port=excluded.port,
                        hostname=excluded.hostname,
                        updated_at=excluded.updated_at
                    ",
                    params![
                        id,
                        name,
                        state,
                        pid.map(i64::from),
                        port.map(i64::from),
                        hostname,
                        ts
                    ],
                )
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!(e))
            }
        })
        .await??;
        Ok(())
    }

    /// Update only the state
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn update_state(&self, id: &str, state: &str) -> anyhow::Result<()> {
        let id = id.to_string();
        let state = state.to_string();
        tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || {
                let ts = now_ts();
                let conn = conn.blocking_lock();
                conn.execute(
                    "UPDATE services SET state=?1, updated_at=?2 WHERE id=?3",
                    params![state, ts, id],
                )
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!(e))
            }
        })
        .await??;
        Ok(())
    }

    /// Update only the pid (can be NULL)
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn update_pid(&self, id: &str, pid: Option<u32>) -> anyhow::Result<()> {
        let id = id.to_string();
        tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || {
                let ts = now_ts();
                let conn = conn.blocking_lock();
                conn.execute(
                    "UPDATE services SET pid=?1, updated_at=?2 WHERE id=?3",
                    params![pid.map(i64::from), ts, id],
                )
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!(e))
            }
        })
        .await??;
        Ok(())
    }

    /// Update only the port (can be NULL)
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn update_port(&self, id: &str, port: Option<u16>) -> anyhow::Result<()> {
        let id = id.to_string();
        tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || {
                let ts = now_ts();
                let conn = conn.blocking_lock();
                conn.execute(
                    "UPDATE services SET port=?1, updated_at=?2 WHERE id=?3",
                    params![port.map(i64::from), ts, id],
                )
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!(e))
            }
        })
        .await??;
        Ok(())
    }

    /// Update only the hostname (can be NULL)
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn update_hostname(&self, id: &str, hostname: Option<&str>) -> anyhow::Result<()> {
        let id = id.to_string();
        let hostname = hostname.map(ToString::to_string);
        tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || {
                let ts = now_ts();
                let conn = conn.blocking_lock();
                conn.execute(
                    "UPDATE services SET hostname=?1, updated_at=?2 WHERE id=?3",
                    params![hostname, ts, id],
                )
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!(e))
            }
        })
        .await??;
        Ok(())
    }

    /// Get current port for a service (if any)
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn fetch_port(&self, id: &str) -> anyhow::Result<Option<u16>> {
        let id = id.to_string();
        let res = tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || -> anyhow::Result<Option<u16>> {
                let val: Option<Option<i64>> = {
                    let conn = conn.blocking_lock();
                    conn.query_row(
                        "SELECT port FROM services WHERE id=?1",
                        params![id],
                        |row| row.get::<_, Option<i64>>(0),
                    )
                    .optional()
                    .map_err(|e| anyhow::anyhow!(e))?
                };
                val.flatten().map_or_else(
                    || Ok(None),
                    |v| {
                        u16::try_from(v)
                            .map(Some)
                            .map_err(|_| anyhow::anyhow!("Invalid port value in database: {v}"))
                    },
                )
            }
        })
        .await??;
        Ok(res)
    }

    /// Get current hostname for a service (if any)
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn fetch_hostname(&self, id: &str) -> anyhow::Result<Option<String>> {
        let id = id.to_string();
        let res = tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || -> anyhow::Result<Option<String>> {
                let val: Option<Option<String>> = {
                    let conn = conn.blocking_lock();
                    conn.query_row(
                        "SELECT hostname FROM services WHERE id=?1",
                        params![id],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()
                    .map_err(|e| anyhow::anyhow!(e))?
                };
                Ok(val.flatten())
            }
        })
        .await??;
        Ok(res)
    }
}

fn now_ts() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    i64::try_from(secs).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;
    use tokio::sync::Mutex;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct HomeGuard {
        original_home: Option<String>,
    }

    impl HomeGuard {
        fn new(original_home: Option<String>) -> Self {
            Self { original_home }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            if let Some(home) = &self.original_home {
                std::env::set_var("HOME", home);
            } else {
                std::env::remove_var("HOME");
            }
        }
    }

    #[tokio::test]
    async fn open_default_requires_home() {
        let _lock = ENV_LOCK.lock().await;
        let original_home = std::env::var("HOME").ok();
        std::env::remove_var("HOME");
        let _home_guard = HomeGuard::new(original_home);

        let err = SqliteStorage::open_default().expect_err("missing HOME should error");
        assert!(err.to_string().contains("HOME"));
    }

    #[tokio::test]
    async fn fetch_port_errors_on_invalid_value() {
        let _lock = ENV_LOCK.lock().await;
        let original_home = std::env::var("HOME").ok();
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("create home dir");
        std::env::set_var("HOME", &home);
        let _home_guard = HomeGuard::new(original_home);

        let storage = SqliteStorage::open_default().expect("open sqlite");
        let conn = storage.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT INTO services (id, name, state, pid, port, hostname, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "svc",
                    "svc",
                    "idle",
                    Option::<i64>::None,
                    70_000_i64,
                    Option::<String>::None,
                    now_ts(),
                ],
            )
            .expect("insert row");
        })
        .await
        .expect("insert task");

        let err = storage.fetch_port("svc").await.expect_err("invalid port");
        assert!(err.to_string().contains("Invalid port value"));
    }
}
