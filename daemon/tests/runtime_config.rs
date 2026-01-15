#![allow(unused_crate_dependencies)]

//! Integration tests for daemon runtime configuration

use std::env;
use std::time::Duration;

use daemon::bootstrap::bootstrap_with_runtime;
use daemon::storage::SqliteStorage;
use ipc::uds_client::JsonRpcClient;

#[tokio::test]
async fn test_runtime_preload_and_ipc_list() {
    let timeout = Duration::from_secs(30);
    tokio::time::timeout(timeout, async {
        // Isolate HOME for SQLite and CANOPUS_IPC_SOCKET for UDS
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        env::set_var("HOME", &home);

        let socket_path = tmp.path().join("canopus.sock");
        env::set_var("CANOPUS_IPC_SOCKET", &socket_path);

        // Resolve example config paths relative to the workspace root (daemon crate is one level deep)
        let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = crate_dir.parent().expect("workspace root");
        let services_path = workspace_root.join("examples/services.toml");
        let runtime_path = workspace_root.join("examples/runtime.toml");

        // Bootstrap daemon components with both configs (without binding port 80)
        let boot = bootstrap_with_runtime(
            Some(services_path.clone()),
            Some(runtime_path.clone()),
            None,
        )
        .await
        .expect("bootstrap_with_runtime");

        // Give the IPC server a brief moment to bind
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Connect to UDS and list services
        let uds = JsonRpcClient::new(&socket_path, None);
        let services = uds.list().await.expect("uds list");
        let ids: std::collections::HashSet<_> = services.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains("web"), "expected 'web' in services: {:?}", ids);
        assert!(ids.contains("api"), "expected 'api' in services: {:?}", ids);

        // Verify runtime preload persisted hostname/port for known services
        let store = SqliteStorage::open_default().expect("open sqlite");

        let web_port = store.fetch_port("web").await.expect("fetch web port");
        let web_host = store.fetch_hostname("web").await.expect("fetch web host");
        assert_eq!(web_port, Some(8000));
        assert_eq!(web_host.as_deref(), Some("web.local"));

        let _api_port = store.fetch_port("api").await.expect("fetch api port");
        let api_host = store.fetch_hostname("api").await.expect("fetch api host");
        // api port is optional in the example runtime.toml
        assert_eq!(api_host.as_deref(), Some("api.local"));

        // Cleanup
        boot.shutdown().await;
    })
    .await
    .expect("test timed out after 30s");
}
