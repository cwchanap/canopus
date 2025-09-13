#![allow(unused_crate_dependencies)]
//! E2E: IPC control methods (bindHost, assignPort)

use ipc::uds_client::JsonRpcClient;

#[tokio::test]
async fn e2e_ipc_bind_assign() {
    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().to_path_buf();

    // UDS + state
    let sock_path = base.join("canopus.sock");
    let state_path = base.join("state.json");
    std::env::set_var("CANOPUS_IPC_SOCKET", &sock_path);
    std::env::set_var("CANOPUS_STATE_FILE", &state_path);

    // Minimal bootstrap with no services is fine
    let boot = daemon::bootstrap::bootstrap(None).await.expect("bootstrap");

    // Wait for IPC socket
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(3);
        while !sock_path.exists() {
            if Instant::now() >= deadline { panic!("uds not ready"); }
            sleep(Duration::from_millis(25)).await;
        }
    }

    let client = JsonRpcClient::new(&sock_path, None);

    // Version
    let _ = client.version().await.expect("version");
    // Bind host no-op
    client.bind_host("nonexistent", "svc.local").await.expect("bindHost");
    // Assign port
    let p = client.assign_port("nonexistent", None).await.expect("assignPort");
    assert!(p == 0 || (p >= 30000 && p <= 60000), "port {} should be in default range or 0 on failure", p);

    boot.shutdown().await;
}

