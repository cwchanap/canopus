#![allow(unused_crate_dependencies)]
//! E2E: IPC control methods (bindHost, assignPort)

use ipc::uds_client::JsonRpcClient;
use std::time::Duration;
pub mod common;

#[tokio::test]
async fn e2e_ipc_bind_assign() {
    common::run_with_timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().expect("tempdir");
        let base = temp.path().to_path_buf();

        // UDS + state
        let sock_path = base.join("canopus.sock");
        let state_path = base.join("state.json");
        std::env::set_var("CANOPUS_IPC_SOCKET", &sock_path);
        std::env::set_var("CANOPUS_STATE_FILE", &state_path);

        // Minimal bootstrap with no services without binding port 80
        let boot = daemon::bootstrap::bootstrap_with_runtime(None, None, None)
            .await
            .expect("bootstrap");

        // Wait for IPC socket
        {
            use tokio::time::{sleep, Duration, Instant};
            let deadline = Instant::now() + Duration::from_secs(3);
            while !sock_path.exists() {
                assert!(Instant::now() < deadline, "uds not ready");
                sleep(Duration::from_millis(25)).await;
            }
        }

        let client = JsonRpcClient::new(&sock_path, None);

        // Version
        let _ = client.version().await.expect("version");
        // Bind host no-op
        client
            .bind_host("nonexistent", "svc.local")
            .await
            .expect("bindHost");
        // Assign port
        let p = client
            .assign_port("nonexistent", None)
            .await
            .expect("assignPort");
        assert!(
            (30000..=60000).contains(&p),
            "port {p} should be in default range"
        );

        boot.shutdown();
    })
    .await;
}
