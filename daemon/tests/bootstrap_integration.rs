use std::fs;
// Silence unused crate dependency lints for workspace-wide dev deps
use canopus_core as _;
use clap as _;
use ipc as _;
use schema as _;
use serde_json as _;
use thiserror as _;
use tracing as _;
use tracing_subscriber as _;

#[tokio::test]
async fn bootstrap_start_stop() {
    let timeout = std::time::Duration::from_secs(30);
    tokio::time::timeout(timeout, async {
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

        // Bootstrap
        let handle = daemon::bootstrap::bootstrap(Some(path))
            .await
            .expect("bootstrap should succeed");
        assert_eq!(handle.services.len(), 1);

        // Trigger shutdown
        handle.shutdown().await;
    })
    .await
    .expect("test timed out after 30s");
}
