use std::env;
use std::time::Duration;

use schema::ServiceState;
use cli::{Client, Result as CliResult};
use daemon::bootstrap::bootstrap_with_runtime;
use schema::ClientConfig;

#[tokio::test]
async fn test_cli_start_with_config_reaches_ready() -> CliResult<()> {
    let timeout = Duration::from_secs(60);
    tokio::time::timeout(timeout, async move {
        // Isolate HOME for SQLite and CANOPUS_IPC_SOCKET for UDS
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        env::set_var("HOME", &home);

        let socket_path = tmp.path().join("canopus.sock");
        env::set_var("CANOPUS_IPC_SOCKET", &socket_path);

        // Resolve example config paths relative to the workspace root
        let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = crate_dir.parent().expect("workspace root");
        let services_path = workspace_root.join("examples/services.toml");
        let runtime_path = workspace_root.join("examples/runtime.toml");

        // Bootstrap supervisors + IPC server (UDS)
        let boot = bootstrap_with_runtime(Some(services_path.clone()), None)
            .await
            .expect("bootstrap_with_runtime");

        // Start a TCP daemon server on a test port so CLI.start() can succeed
        let daemon_cfg = schema::DaemonConfig { host: "127.0.0.1".into(), port: 49384, ..Default::default() };
        let daemon = daemon::Daemon::new(daemon_cfg);
        let daemon_handle = tokio::spawn(async move { let _ = daemon.start().await; });

        // Give servers a brief moment to bind
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Run CLI start_with_config which should start services per runtime.toml and wait for Ready
        let client_cfg = ClientConfig { daemon_host: "127.0.0.1".into(), daemon_port: 49384, timeout_seconds: 15 };
        let client = Client::new(client_cfg);
        client.start_with_config(Some(runtime_path.clone())).await?;

        // Verify services reached Ready and have PIDs
        let uds = ipc::uds_client::JsonRpcClient::new(&socket_path, None);
        for id in ["web", "api"] {
            let detail = uds.status(id).await.expect("status");
            assert!(matches!(detail.state, ServiceState::Ready | ServiceState::Starting | ServiceState::Spawning));
            // If not Ready yet, give it a little more time
            if detail.state != ServiceState::Ready {
                tokio::time::sleep(Duration::from_millis(300)).await;
                let detail2 = uds.status(id).await.expect("status2");
                assert_eq!(detail2.state, ServiceState::Ready, "{} should become Ready", id);
            }
            assert!(detail.pid.is_some(), "{} should have a PID after start", id);
        }

        // Cleanup
        boot.shutdown().await;
        daemon_handle.abort();
        Ok::<(), cli::CliError>(())
    })
    .await
    .expect("test timed out after 60s")?;
    Ok(())
}
