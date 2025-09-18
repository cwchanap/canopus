//! SQLite storage for daemon runtime state
//!
//! Stores service state and PID persistently in $HOME/.canopus/canopus.db

use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use ipc::Result as IpcResult;
use ipc::IpcError;

#[derive(Clone)]
pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
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
}

impl SqliteStorage {
    /// Open or create the SQLite database at $HOME/.canopus/canopus.db
    pub fn open_default() -> anyhow::Result<Self> {
        let base = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let dir = base.join(".canopus");
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("canopus.db");

        let conn = Connection::open(db_path)?;
        // Enable WAL for better concurrency and durability
        conn.pragma_update(None, "journal_mode", &"WAL")
            .ok();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS services (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                state TEXT NOT NULL,
                pid INTEGER,
                port INTEGER,
                hostname TEXT,
                updated_at INTEGER NOT NULL
            );
            "#,
        )?;

        // Best-effort migrations for existing DBs missing new columns
        let _ = conn.execute("ALTER TABLE services ADD COLUMN port INTEGER", []);
        let _ = conn.execute("ALTER TABLE services ADD COLUMN hostname TEXT", []);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Seed or upsert a service row
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
        let hostname = hostname.map(|s| s.to_string());
        tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || {
                let ts = now_ts();
                let conn = conn.blocking_lock();
                conn.execute(
                    r#"
                    INSERT INTO services (id, name, state, pid, port, hostname, updated_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    ON CONFLICT(id) DO UPDATE SET
                        name=excluded.name,
                        state=excluded.state,
                        pid=excluded.pid,
                        port=excluded.port,
                        hostname=excluded.hostname,
                        updated_at=excluded.updated_at
                    "#,
                    params![id, name, state, pid.map(|p| p as i64), port.map(|p| p as i64), hostname, ts],
                )
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!(e))
            }
        })
        .await??;
        Ok(())
    }

    /// Update only the state
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
    pub async fn update_pid(&self, id: &str, pid: Option<u32>) -> anyhow::Result<()> {
        let id = id.to_string();
        tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || {
                let ts = now_ts();
                let conn = conn.blocking_lock();
                conn.execute(
                    "UPDATE services SET pid=?1, updated_at=?2 WHERE id=?3",
                    params![pid.map(|p| p as i64), ts, id],
                )
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!(e))
            }
        })
        .await??;
        Ok(())
    }

    /// Update only the port (can be NULL)
    pub async fn update_port(&self, id: &str, port: Option<u16>) -> anyhow::Result<()> {
        let id = id.to_string();
        tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || {
                let ts = now_ts();
                let conn = conn.blocking_lock();
                conn.execute(
                    "UPDATE services SET port=?1, updated_at=?2 WHERE id=?3",
                    params![port.map(|p| p as i64), ts, id],
                )
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!(e))
            }
        })
        .await??;
        Ok(())
    }

    /// Update only the hostname (can be NULL)
    pub async fn update_hostname(&self, id: &str, hostname: Option<&str>) -> anyhow::Result<()> {
        let id = id.to_string();
        let hostname = hostname.map(|s| s.to_string());
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
    pub async fn fetch_port(&self, id: &str) -> anyhow::Result<Option<u16>> {
        let id = id.to_string();
        let res = tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || -> anyhow::Result<Option<u16>> {
                let conn = conn.blocking_lock();
                let mut stmt = conn.prepare("SELECT port FROM services WHERE id=?1").map_err(|e| anyhow::anyhow!(e))?;
                let val: Option<Option<i64>> = stmt
                    .query_row(params![id], |row| row.get::<_, Option<i64>>(0))
                    .optional()
                    .map_err(|e| anyhow::anyhow!(e))?;
                Ok(val.flatten().map(|v| v as u16))
            }
        })
        .await??;
        Ok(res)
    }

    /// Get current hostname for a service (if any)
    pub async fn fetch_hostname(&self, id: &str) -> anyhow::Result<Option<String>> {
        let id = id.to_string();
        let res = tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || -> anyhow::Result<Option<String>> {
                let conn = conn.blocking_lock();
                let mut stmt = conn.prepare("SELECT hostname FROM services WHERE id=?1").map_err(|e| anyhow::anyhow!(e))?;
                let val: Option<Option<String>> = stmt
                    .query_row(params![id], |row| row.get::<_, Option<String>>(0))
                    .optional()
                    .map_err(|e| anyhow::anyhow!(e))?;
                Ok(val.flatten())
            }
        })
        .await??;
        Ok(res)
    }
}

fn now_ts() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
