//! Integration tests for daemon bootstrap functionality

use std::env;
use std::fs;
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
async fn bootstrap_start_stop() {
    let timeout = std::time::Duration::from_secs(30);
    tokio::time::timeout(timeout, async {
        // Isolate HOME for SQLite storage in sandboxed test environments
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        env::set_var("HOME", &home);
        let socket_path = tmp.path().join("canopus.sock");
        env::set_var("CANOPUS_IPC_SOCKET", &socket_path);

        // Prepare a minimal services file
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("services.toml");
        let toml = r#"
            [[services]]
            id = "svc-demo"
            name = "Demo"
            command = "/bin/sh"
            args = ["-c", "sleep 1"]
        "#;
        fs::write(&path, toml).unwrap();

        // Bootstrap without binding port 80
        let handle = daemon::bootstrap::bootstrap_with_runtime(Some(path), None, None)
            .await
            .expect("bootstrap should succeed");
        assert_eq!(handle.services.len(), 1);

        // Trigger shutdown
        handle.shutdown();
    })
    .await
    .expect("test timed out after 30s");
}
