#![allow(unused_crate_dependencies)]
use std::env;
use std::time::Duration;

use cli::{Client, Result as CliResult};
use daemon::bootstrap::bootstrap_with_runtime;
use daemon::storage::SqliteStorage;
use schema::ClientConfig;
use schema::ServiceState;

#[tokio::test]
async fn test_cli_start_with_config_removes_unlisted_services() -> CliResult<()> {
    let timeout = Duration::from_secs(45);
    tokio::time::timeout(timeout, async move {
        // Isolate HOME for SQLite and CANOPUS_IPC_SOCKET for UDS
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        env::set_var("HOME", &home);

        let socket_path = tmp.path().join("canopus.sock");
        env::set_var("CANOPUS_IPC_SOCKET", &socket_path);

        // Resolve example config paths relative to workspace root
        let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = crate_dir.parent().expect("workspace root");
        let services_path = workspace_root.join("examples/services.toml");

        // Create a runtime config that lists only an unknown service to force deletion of known ones
        let runtime_path = tmp.path().join("runtime.toml");
        std::fs::write(&runtime_path, "[bogus]\nhostname=\"x\"\nport=5000\n").unwrap();

        // Bootstrap supervisors + IPC server (UDS) without binding port 80
        let boot = bootstrap_with_runtime(Some(services_path.clone()), None, None)
            .await
            .expect("bootstrap_with_runtime");

        // Start a TCP daemon server on a test port so CLI.start() can succeed
        let daemon_cfg = schema::DaemonConfig {
            host: "127.0.0.1".into(),
            port: 49384,
            ..Default::default()
        };
        let daemon = daemon::Daemon::new(daemon_cfg);
        let daemon_handle = tokio::spawn(async move {
            let _ = daemon.start().await;
        });

        // Give servers a brief moment to bind
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Run CLI start_with_config which should stop and delete unlisted services
        let client_cfg = ClientConfig {
            daemon_host: "127.0.0.1".into(),
            daemon_port: 49384,
            timeout_seconds: 5,
        };
        let client = Client::new(client_cfg);
        client.start_with_config(Some(runtime_path.clone())).await?;

        // Verify DB rows for known services are removed
        let store = SqliteStorage::open_default().expect("open sqlite");
        let web_port = store.fetch_port("web").await.expect("fetch web port");
        let api_port = store.fetch_port("api").await.expect("fetch api port");
        assert_eq!(web_port, None, "web port should be None after deletion");
        assert_eq!(api_port, None, "api port should be None after deletion");

        // Also verify services are no longer running (should be Idle after stop)
        let uds = ipc::uds_client::JsonRpcClient::new(&socket_path, None);
        let web = uds.status("web").await.expect("status web");
        let api = uds.status("api").await.expect("status api");
        assert_eq!(web.state, ServiceState::Idle);
        assert_eq!(api.state, ServiceState::Idle);

        // Cleanup
        boot.shutdown().await;
        daemon_handle.abort();
        Ok::<(), cli::CliError>(())
    })
    .await
    .expect("test timed out after 45s")?;
    Ok(())
}
