//! SQLite storage for daemon runtime state
//!
//! Stores service state and PID persistently in $HOME/.canopus/canopus.db

use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
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
                updated_at INTEGER NOT NULL
            );
            "#,
        )?;

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
    ) -> anyhow::Result<()> {
        let id = id.to_string();
        let name = name.to_string();
        let state = state.to_string();
        tokio::task::spawn_blocking({
            let conn = self.conn.clone();
            move || {
                let ts = now_ts();
                let conn = conn.blocking_lock();
                conn.execute(
                    r#"
                    INSERT INTO services (id, name, state, pid, updated_at)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    ON CONFLICT(id) DO UPDATE SET
                        name=excluded.name,
                        state=excluded.state,
                        pid=excluded.pid,
                        updated_at=excluded.updated_at
                    "#,
                    params![id, name, state, pid.map(|p| p as i64), ts],
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
}

fn now_ts() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
