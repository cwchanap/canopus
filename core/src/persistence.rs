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
use tracing::warn;

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
    #[must_use]
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
#[must_use]
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
///
/// # Errors
///
/// Returns an error if the snapshot file cannot be read or parsed.
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

    let snap: RegistrySnapshot =
        serde_json::from_str(&buf).map_err(CoreError::SerializationError)?;

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
///
/// # Errors
///
/// Returns an error if the snapshot cannot be written to disk.
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
            .map_err(|e| {
                CoreError::IoError(std::io::Error::new(
                    e.kind(),
                    format!("Failed to open temp snapshot {}: {}", tmp_path.display(), e),
                ))
            })?;
        f.write_all(&json).map_err(|e| {
            CoreError::IoError(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to write temp snapshot {}: {}",
                    tmp_path.display(),
                    e
                ),
            ))
        })?;
        if let Err(e) = f.flush() {
            warn!(
                "Failed to flush snapshot file {}: {}. Data may not be durable.",
                tmp_path.display(),
                e
            );
        }
        // Best-effort durability
        if let Err(e) = f.sync_all() {
            warn!(
                "Failed to sync snapshot file {}: {}. Data may not be durable.",
                tmp_path.display(),
                e
            );
        }
    }

    // Rename over the destination atomically
    fs::rename(&tmp_path, path).map_err(|e| {
        CoreError::IoError(std::io::Error::new(
            e.kind(),
            format!(
                "Failed to replace snapshot {} with {}: {}",
                path.display(),
                tmp_path.display(),
                e
            ),
        ))
    })?;

    // Best-effort fsync of directory to persist rename
    if let Some(parent) = path.parent() {
        match File::open(parent) {
            Ok(dir) => {
                if let Err(e) = dir.sync_all() {
                    warn!(
                        "Failed to sync directory {}: {}. Rename may not be durable.",
                        parent.display(),
                        e
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to open directory {} for sync: {}",
                    parent.display(),
                    e
                );
            }
        }
    }

    Ok(())
}

/// Create a current timestamp string in RFC3339 format (seconds precision)
#[must_use]
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
    use schema::BackoffConfig;
    use std::collections::HashMap;
    use std::io::ErrorKind;
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
                    environment: HashMap::default(),
                    working_directory: None,
                    route: None,
                    restart_policy: schema::RestartPolicy::Always,
                    backoff_config: BackoffConfig::default(),
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
        let msg = format!("{err}");
        assert!(msg.contains("Serialization error"));
    }

    #[test]
    fn load_missing_file_returns_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let err = load_snapshot(&path).unwrap_err();
        match err {
            CoreError::IoError(inner) => {
                assert_eq!(inner.kind(), ErrorKind::NotFound);
            }
            other => panic!("expected IoError(NotFound), got: {other}"),
        }
    }

    #[test]
    fn wrong_snapshot_version_is_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");

        // Write a snapshot with an unsupported version
        let wrong_version = serde_json::json!({
            "version": 999,
            "timestamp": "2024-01-01T00:00:00Z",
            "services": []
        });
        fs::write(&path, wrong_version.to_string()).unwrap();

        let err = load_snapshot(&path).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Unsupported snapshot version"));
        assert!(msg.contains("999"));
    }

    #[test]
    fn empty_snapshot_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");

        let snap = RegistrySnapshot::empty();
        assert_eq!(snap.version, SNAPSHOT_VERSION);
        assert!(snap.services.is_empty());

        write_snapshot_atomic(&path, &snap).expect("write ok");
        let loaded = load_snapshot(&path).expect("read ok");

        assert_eq!(loaded.version, SNAPSHOT_VERSION);
        assert!(loaded.services.is_empty());
    }

    #[test]
    fn current_timestamp_is_rfc3339_format() {
        let ts = current_timestamp();
        // RFC3339 timestamps end with 'Z' (UTC) and contain 'T' separator
        assert!(ts.ends_with('Z'), "timestamp should end with Z: {ts}");
        assert!(
            ts.contains('T'),
            "timestamp should contain T separator: {ts}"
        );
        // Basic length check: "2024-01-01T00:00:00Z" = 20 chars
        assert!(ts.len() >= 20, "timestamp too short: {ts}");
    }

    #[test]
    fn atomic_write_creates_parent_directory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("state.json");

        // Parent does not exist yet
        assert!(!path.parent().unwrap().exists());

        let snap = RegistrySnapshot::empty();
        write_snapshot_atomic(&path, &snap).expect("should create parent dirs");
        assert!(path.exists());
    }

    #[test]
    fn snapshot_preserves_all_fields() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");

        let snap = make_snap();
        write_snapshot_atomic(&path, &snap).expect("write ok");

        let loaded = load_snapshot(&path).expect("read ok");
        let svc = &loaded.services[0];

        assert_eq!(svc.id, "svc");
        assert_eq!(svc.spec.name, "Test");
        assert_eq!(svc.spec.command, "echo");
        assert_eq!(svc.last_state, ServiceState::Idle);
        assert!(svc.last_pid.is_none());
    }

    #[test]
    fn snapshot_with_pid_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");

        let mut snap = make_snap();
        snap.services[0].last_pid = Some(12345);
        snap.services[0].last_state = ServiceState::Ready;

        write_snapshot_atomic(&path, &snap).expect("write ok");
        let loaded = load_snapshot(&path).expect("read ok");

        assert_eq!(loaded.services[0].last_pid, Some(12345));
        assert_eq!(loaded.services[0].last_state, ServiceState::Ready);
    }

    /// RAII guard that restores an env var on drop, preventing test pollution.
    struct EnvVarGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn default_snapshot_path_uses_canopus_state_file_env() {
        let custom_path = "/tmp/my_custom_state.json";
        let _guard = EnvVarGuard::set("CANOPUS_STATE_FILE", custom_path);
        let path = default_snapshot_path();
        assert_eq!(path.to_str().unwrap(), custom_path);
    }

    #[test]
    fn registry_snapshot_clone_equals_original() {
        let snap = make_snap();
        let cloned = snap.clone();
        assert_eq!(snap, cloned);
    }

    #[test]
    fn registry_snapshot_partial_eq() {
        let snap1 = make_snap();
        // Produce a second snapshot with an identical timestamp so PartialEq holds fully.
        let mut snap2 = make_snap();
        snap2.timestamp = snap1.timestamp.clone();
        assert_eq!(snap1, snap2);
    }

    #[test]
    fn service_snapshot_last_pid_none_is_not_serialized() {
        let snap = make_snap();
        let json = serde_json::to_string(&snap.services[0]).unwrap();
        // When last_pid is None, skip_serializing_if applies and key should be absent
        assert!(
            !json.contains("lastPid"),
            "lastPid should not be present when None: {json}"
        );
    }

    #[test]
    fn service_snapshot_last_pid_some_is_serialized() {
        let mut snap = make_snap();
        snap.services[0].last_pid = Some(9999);
        let json = serde_json::to_string(&snap.services[0]).unwrap();
        assert!(
            json.contains("lastPid"),
            "lastPid should be present when Some: {json}"
        );
        assert!(json.contains("9999"));
    }

    #[test]
    fn current_timestamp_format_and_progression() {
        let t1 = current_timestamp();
        assert!(!t1.is_empty(), "timestamp should not be empty");
        assert!(t1.ends_with('Z'), "timestamp should end with Z: {t1}");
        assert!(
            t1.contains('T'),
            "timestamp should contain T separator: {t1}"
        );

        // Poll until the timestamp changes or a 1s timeout is reached.
        let start = std::time::Instant::now();
        let mut t2 = String::new();
        loop {
            t2 = current_timestamp();
            if t2 != t1 {
                break;
            }
            if start.elapsed() >= std::time::Duration::from_secs(1) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!t2.is_empty(), "second timestamp should not be empty");
        assert!(
            t2.ends_with('Z'),
            "second timestamp should end with Z: {t2}"
        );
        assert_ne!(
            t1, t2,
            "timestamps should differ within 1s timeout (t1={t1}, t2={t2})"
        );
    }
}
