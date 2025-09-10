//! Persistence: versioned JSON snapshot storage with atomic writes
//!
//! Provides a file-based registry snapshot for service specifications and
//! last known state. Writes are crash-safe via write-to-temp + fsync + rename.
//! Reads validate version and structure; corrupted files surface errors so the
//! daemon can recover with a clean state.

use crate::{CoreError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use schema::{ServiceSpec, ServiceState};

/// Snapshot file format version
pub const SNAPSHOT_VERSION: u32 = 1;

/// Full registry snapshot written to disk
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySnapshot {
    /// Format version
    pub version: u32,
    /// RFC3339 timestamp when this snapshot was produced
    pub timestamp: String,
    /// Per-service entries
    pub services: Vec<ServiceSnapshot>,
}

/// Per-service snapshot containing spec and last known state info
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSnapshot {
    /// Service identifier
    pub id: String,
    /// Full service specification
    pub spec: ServiceSpec,
    /// Last known state
    pub last_state: ServiceState,
    /// Last known pid if process was started (not adopted on recovery)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_pid: Option<u32>,
}

impl RegistrySnapshot {
    /// Create an empty snapshot
    pub fn empty() -> Self {
        Self {
            version: SNAPSHOT_VERSION,
            timestamp: current_timestamp(),
            services: Vec::new(),
        }
    }
}

/// Return a default snapshot path.
///
/// Order:
/// - `CANOPUS_STATE_FILE` env var if provided
/// - `$HOME/.canopus/state.json` if HOME exists
/// - `./canopus_state.json` otherwise
pub fn default_snapshot_path() -> PathBuf {
    if let Ok(p) = std::env::var("CANOPUS_STATE_FILE") {
        return PathBuf::from(p);
    }
    if let Some(home) = dirs_next::home_dir() {
        return home.join(".canopus").join("state.json");
    }
    PathBuf::from("canopus_state.json")
}

/// Load a registry snapshot from file.
///
/// Returns `Err` for I/O or parse errors so callers can recover by ignoring
/// the snapshot and starting clean.
pub fn load_snapshot(path: impl AsRef<Path>) -> Result<RegistrySnapshot> {
    let path = path.as_ref();
    let mut file = File::open(path).map_err(|e| {
        CoreError::IoError(std::io::Error::new(
            e.kind(),
            format!("Failed to open snapshot {}: {}", path.display(), e),
        ))
    })?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).map_err(|e| {
        CoreError::IoError(std::io::Error::new(
            e.kind(),
            format!("Failed to read snapshot {}: {}", path.display(), e),
        ))
    })?;

    let snap: RegistrySnapshot = serde_json::from_str(&buf).map_err(CoreError::SerializationError)?;

    if snap.version != SNAPSHOT_VERSION {
        return Err(CoreError::ValidationError(format!(
            "Unsupported snapshot version {} (expected {})",
            snap.version, SNAPSHOT_VERSION
        )));
    }

    Ok(snap)
}

/// Atomically write a registry snapshot to file.
///
/// Steps:
/// - Ensure parent directory exists and fsync it if needed
/// - Write JSON to a temp file in the same directory
/// - `flush` + `sync_all` on the temp file
/// - `rename` temp file over the destination
/// - Best-effort fsync of the directory to persist rename
pub fn write_snapshot_atomic(path: impl AsRef<Path>, snap: &RegistrySnapshot) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CoreError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to create snapshot dir {}: {}", parent.display(), e),
            ))
        })?;
    }

    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(snap)?;

    {
        // Write to temp file with truncation
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)
            .map_err(|e| CoreError::IoError(std::io::Error::new(e.kind(), format!(
                "Failed to open temp snapshot {}: {}", tmp_path.display(), e
            ))))?;
        f.write_all(&json).map_err(|e| CoreError::IoError(std::io::Error::new(e.kind(), format!(
            "Failed to write temp snapshot {}: {}", tmp_path.display(), e
        ))))?;
        f.flush().ok();
        // Best-effort durability
        let _ = f.sync_all();
    }

    // Rename over the destination atomically
    fs::rename(&tmp_path, path).map_err(|e| {
        CoreError::IoError(std::io::Error::new(
            e.kind(),
            format!(
                "Failed to replace snapshot {} with {}: {}",
                path.display(), tmp_path.display(), e
            ),
        ))
    })?;

    // Best-effort fsync of directory to persist rename
    if let Some(parent) = path.parent() {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }

    Ok(())
}

/// Create a current timestamp string in RFC3339 format (seconds precision)
pub fn current_timestamp() -> String {
    format!(
        "{}Z",
        humantime::format_rfc3339_seconds(std::time::SystemTime::now())
            .to_string()
            .trim_end_matches(".000000000Z")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_snap() -> RegistrySnapshot {
        RegistrySnapshot {
            version: SNAPSHOT_VERSION,
            timestamp: current_timestamp(),
            services: vec![ServiceSnapshot {
                id: "svc".into(),
                spec: ServiceSpec {
                    id: "svc".into(),
                    name: "Test".into(),
                    command: "echo".into(),
                    args: vec!["hello".into()],
                    environment: Default::default(),
                    working_directory: None,
                    restart_policy: schema::RestartPolicy::Always,
                    backoff_config: Default::default(),
                    health_check: None,
                    readiness_check: None,
                    graceful_timeout_secs: 30,
                    startup_timeout_secs: 60,
                },
                last_state: ServiceState::Idle,
                last_pid: None,
            }],
        }
    }

    #[test]
    fn roundtrip_atomic_write_and_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");

        let snap = make_snap();
        write_snapshot_atomic(&path, &snap).expect("write ok");

        let loaded = load_snapshot(&path).expect("read ok");
        assert_eq!(loaded.version, SNAPSHOT_VERSION);
        assert_eq!(loaded.services.len(), 1);
        assert_eq!(loaded.services[0].id, "svc");
        assert_eq!(loaded.services[0].last_state, ServiceState::Idle);
    }

    #[test]
    fn corrupted_file_is_reported() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");

        // Write valid
        let snap = make_snap();
        write_snapshot_atomic(&path, &snap).expect("write ok");

        // Corrupt by overwriting with partial content
        fs::write(&path, b"{ invalid json").unwrap();

        let err = load_snapshot(&path).unwrap_err();
        // Must surface a serialization/validation error
        let msg = format!("{}", err);
        assert!(msg.contains("Serialization error"));
    }
}
