//! Integration tests for daemon recovery functionality

use daemon::bootstrap;
use std::env;
use std::fs;
use std::time::Duration;
// Silence unused crate dependency lints for workspace-wide dev deps
use anyhow as _;
use async_trait as _;
use canopus_core as _;
use clap as _;
use ipc as _;
use rusqlite as _;
use schema as _;
use serde_json as _;
use thiserror as _;
use tracing as _;
use tracing_subscriber as _;

#[tokio::test]
async fn corrupted_snapshot_does_not_prevent_bootstrap() {
    let dir = tempfile::tempdir().unwrap();
    let snap_path = dir.path().join("state.json");
    fs::write(&snap_path, b"{ not json").unwrap();
    env::set_var("CANOPUS_STATE_FILE", &snap_path);
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    env::set_var("HOME", &home);

    // Minimal services file in temp
    let cfg_dir = tempfile::tempdir().unwrap();
    let cfg_path = cfg_dir.path().join("services.toml");
    fs::write(
        &cfg_path,
        r#"
        [[services]]
        id = "svc1"
        name = "Service One"
        command = "echo"
        args = ["hello"]
        restartPolicy = "never"
        "#,
    )
    .unwrap();

    let handle = bootstrap::bootstrap_with_runtime(Some(cfg_path), None, None)
        .await
        .expect("bootstrap ok");
    assert_eq!(handle.services.len(), 1);

    // Give snapshot writer a moment to run and write a clean file
    tokio::time::sleep(Duration::from_millis(100)).await;
    let contents = fs::read_to_string(&snap_path).unwrap();
    assert!(contents.contains("\"version\""));

    handle.shutdown();
}
