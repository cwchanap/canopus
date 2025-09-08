use std::fs;

#[tokio::test]
async fn bootstrap_start_stop() {
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
    let handle = daemon::bootstrap::bootstrap(Some(path)).await.expect("bootstrap should succeed");
    assert_eq!(handle.services.len(), 1);

    // Trigger shutdown
    handle.shutdown().await;
}

