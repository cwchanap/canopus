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

    /// Create a `SqliteStorage` backed by an in-memory `SQLite` database for testing.
    /// This avoids touching the filesystem or environment variables.
    fn open_in_memory() -> SqliteStorage {
        use rusqlite::Connection;
        let conn = Connection::open_in_memory().expect("in-memory db");
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
        )
        .expect("schema");
        SqliteStorage {
            conn: Arc::new(Mutex::new(conn)),
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

    #[tokio::test]
    async fn upsert_and_fetch_port() {
        let storage = open_in_memory();
        storage
            .upsert_service("svc1", "Service One", "idle", None, Some(8080), None)
            .await
            .expect("upsert");
        let port = storage.fetch_port("svc1").await.expect("fetch_port");
        assert_eq!(port, Some(8080));
    }

    #[tokio::test]
    async fn upsert_with_null_port_returns_none() {
        let storage = open_in_memory();
        storage
            .upsert_service("svc2", "Service Two", "idle", None, None, None)
            .await
            .expect("upsert");
        let port = storage.fetch_port("svc2").await.expect("fetch_port");
        assert_eq!(port, None);
    }

    #[tokio::test]
    async fn upsert_and_fetch_hostname() {
        let storage = open_in_memory();
        storage
            .upsert_service("svc3", "Service Three", "idle", None, None, Some("myhost"))
            .await
            .expect("upsert");
        let hostname = storage
            .fetch_hostname("svc3")
            .await
            .expect("fetch_hostname");
        assert_eq!(hostname, Some("myhost".to_string()));
    }

    #[tokio::test]
    async fn upsert_with_null_hostname_returns_none() {
        let storage = open_in_memory();
        storage
            .upsert_service("svc4", "Service Four", "idle", None, None, None)
            .await
            .expect("upsert");
        let hostname = storage
            .fetch_hostname("svc4")
            .await
            .expect("fetch_hostname");
        assert_eq!(hostname, None);
    }

    #[tokio::test]
    async fn fetch_port_nonexistent_service_returns_none() {
        let storage = open_in_memory();
        let port = storage
            .fetch_port("does-not-exist")
            .await
            .expect("fetch_port");
        assert_eq!(port, None);
    }

    #[tokio::test]
    async fn fetch_hostname_nonexistent_service_returns_none() {
        let storage = open_in_memory();
        let hostname = storage
            .fetch_hostname("does-not-exist")
            .await
            .expect("fetch_hostname");
        assert_eq!(hostname, None);
    }

    #[tokio::test]
    async fn upsert_is_idempotent_last_write_wins() {
        let storage = open_in_memory();
        storage
            .upsert_service(
                "svc5",
                "Old Name",
                "idle",
                None,
                Some(1000),
                Some("old-host"),
            )
            .await
            .expect("first upsert");
        storage
            .upsert_service(
                "svc5",
                "New Name",
                "running",
                Some(42),
                Some(2000),
                Some("new-host"),
            )
            .await
            .expect("second upsert");
        let port = storage.fetch_port("svc5").await.expect("fetch_port");
        let hostname = storage
            .fetch_hostname("svc5")
            .await
            .expect("fetch_hostname");
        assert_eq!(port, Some(2000));
        assert_eq!(hostname, Some("new-host".to_string()));
    }

    #[tokio::test]
    async fn update_port_changes_value() {
        let storage = open_in_memory();
        storage
            .upsert_service("svc6", "Service Six", "idle", None, Some(9000), None)
            .await
            .expect("upsert");
        storage
            .update_port("svc6", Some(9001))
            .await
            .expect("update_port");
        let port = storage.fetch_port("svc6").await.expect("fetch_port");
        assert_eq!(port, Some(9001));
    }

    #[tokio::test]
    async fn update_port_to_null_clears_value() {
        let storage = open_in_memory();
        storage
            .upsert_service("svc7", "Service Seven", "idle", None, Some(9000), None)
            .await
            .expect("upsert");
        storage
            .update_port("svc7", None)
            .await
            .expect("update_port null");
        let port = storage.fetch_port("svc7").await.expect("fetch_port");
        assert_eq!(port, None);
    }

    #[tokio::test]
    async fn update_hostname_changes_value() {
        let storage = open_in_memory();
        storage
            .upsert_service(
                "svc8",
                "Service Eight",
                "idle",
                None,
                None,
                Some("original"),
            )
            .await
            .expect("upsert");
        storage
            .update_hostname("svc8", Some("updated"))
            .await
            .expect("update_hostname");
        let hostname = storage
            .fetch_hostname("svc8")
            .await
            .expect("fetch_hostname");
        assert_eq!(hostname, Some("updated".to_string()));
    }

    #[tokio::test]
    async fn update_hostname_to_null_clears_value() {
        let storage = open_in_memory();
        storage
            .upsert_service(
                "svc9",
                "Service Nine",
                "idle",
                None,
                None,
                Some("will-be-cleared"),
            )
            .await
            .expect("upsert");
        storage
            .update_hostname("svc9", None)
            .await
            .expect("update_hostname null");
        let hostname = storage
            .fetch_hostname("svc9")
            .await
            .expect("fetch_hostname");
        assert_eq!(hostname, None);
    }

    #[tokio::test]
    async fn update_state_persists() {
        let storage = open_in_memory();
        storage
            .upsert_service("svc10", "Service Ten", "idle", None, None, None)
            .await
            .expect("upsert");
        storage
            .update_state("svc10", "running")
            .await
            .expect("update_state");
        // State column is not exposed via a getter, but we verify indirectly
        // by confirming no error and that port/hostname are unaffected.
        let port = storage.fetch_port("svc10").await.expect("fetch_port");
        assert_eq!(port, None);
    }

    #[tokio::test]
    async fn update_pid_persists() {
        let storage = open_in_memory();
        storage
            .upsert_service("svc11", "Service Eleven", "idle", None, None, None)
            .await
            .expect("upsert");
        storage
            .update_pid("svc11", Some(12345))
            .await
            .expect("update_pid");
        storage
            .update_pid("svc11", None)
            .await
            .expect("update_pid null");
        // PID column has no getter; verify port/hostname are unaffected.
        let port = storage.fetch_port("svc11").await.expect("fetch_port");
        assert_eq!(port, None);
    }

    #[tokio::test]
    async fn delete_service_removes_row() {
        let storage = open_in_memory();
        storage
            .upsert_service(
                "svc12",
                "Service Twelve",
                "idle",
                None,
                Some(8888),
                Some("ghost"),
            )
            .await
            .expect("upsert");
        // Confirm it exists before deletion
        let port_before = storage
            .fetch_port("svc12")
            .await
            .expect("fetch_port before");
        assert_eq!(port_before, Some(8888));

        storage
            .delete_service("svc12")
            .await
            .expect("delete_service");

        let port_after = storage.fetch_port("svc12").await.expect("fetch_port after");
        assert_eq!(port_after, None);
        let hostname_after = storage
            .fetch_hostname("svc12")
            .await
            .expect("fetch_hostname after");
        assert_eq!(hostname_after, None);
    }

    #[tokio::test]
    async fn delete_nonexistent_service_is_noop() {
        let storage = open_in_memory();
        // Deleting a service that doesn't exist should not error
        storage
            .delete_service("ghost-service")
            .await
            .expect("delete nonexistent");
    }

    // --- ServiceMetaStore trait method tests ---

    #[tokio::test]
    async fn service_meta_store_set_and_get_port() {
        use ipc::server::ServiceMetaStore;
        let storage = open_in_memory();
        storage
            .upsert_service("trait-svc", "Trait Service", "idle", None, None, None)
            .await
            .expect("upsert");

        storage
            .set_port("trait-svc", Some(7777))
            .await
            .expect("set_port");
        let port = storage.get_port("trait-svc").await.expect("get_port");
        assert_eq!(port, Some(7777));
    }

    #[tokio::test]
    async fn service_meta_store_set_port_null_clears() {
        use ipc::server::ServiceMetaStore;
        let storage = open_in_memory();
        storage
            .upsert_service(
                "trait-svc2",
                "Trait Service 2",
                "idle",
                None,
                Some(5000),
                None,
            )
            .await
            .expect("upsert");

        storage
            .set_port("trait-svc2", None)
            .await
            .expect("set_port null");
        let port = storage.get_port("trait-svc2").await.expect("get_port");
        assert_eq!(port, None);
    }

    #[tokio::test]
    async fn service_meta_store_set_and_get_hostname() {
        use ipc::server::ServiceMetaStore;
        let storage = open_in_memory();
        storage
            .upsert_service("trait-svc3", "Trait Service 3", "idle", None, None, None)
            .await
            .expect("upsert");

        storage
            .set_hostname("trait-svc3", Some("example.local"))
            .await
            .expect("set_hostname");
        let hostname = storage
            .get_hostname("trait-svc3")
            .await
            .expect("get_hostname");
        assert_eq!(hostname, Some("example.local".to_string()));
    }

    #[tokio::test]
    async fn service_meta_store_delete_removes_row() {
        use ipc::server::ServiceMetaStore;
        let storage = open_in_memory();
        storage
            .upsert_service(
                "trait-svc4",
                "Trait Service 4",
                "idle",
                None,
                Some(4444),
                None,
            )
            .await
            .expect("upsert");

        storage.delete("trait-svc4").await.expect("trait delete");
        let port = storage.get_port("trait-svc4").await.expect("get_port");
        assert_eq!(port, None);
    }

    #[tokio::test]
    async fn now_ts_returns_positive_value() {
        let ts = now_ts();
        assert!(ts > 0, "timestamp should be positive, got {ts}");
    }
}
