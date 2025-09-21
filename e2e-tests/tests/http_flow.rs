#![allow(unused_crate_dependencies)]
//! E2E tests for HTTP service using --port and --hostname flow

use ipc::uds_client::JsonRpcClient;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points to e2e-tests crate directory
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent().expect("workspace root").to_path_buf()
}

fn e2e_http_bin_path() -> PathBuf {
    // Try cargo-provided env (works when workspace builds all bins)
    if let Some(p) = std::env::var_os("CARGO_BIN_EXE_e2e_http") {
        return PathBuf::from(p);
    }
    let root = workspace_root();
    // Ensure the binary is built; ignore failures (will be caught by missing file)
    let _ = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("daemon")
        .arg("--bin")
        .arg("e2e_http")
        .arg("--quiet")
        .current_dir(&root)
        .status();

    // Resolve target dir
    #[cfg(windows)]
    let bin = root.join("target\\debug\\e2e_http.exe");
    #[cfg(not(windows))]
    let bin = root.join("target/debug/e2e_http");
    if bin.exists() {
        bin
    } else {
        panic!("Unable to locate e2e_http binary at {}", bin.display())
    }
}

fn uds_socket_path(label: &str) -> PathBuf {
    // Keep under /tmp to avoid long UDS path issues on macOS (sun_path ~104 bytes)
    let pid = std::process::id();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let filename = format!("canopus-{}-{}-{}.sock", label, pid, ts);
    PathBuf::from("/tmp").join(filename)
}

fn make_services_toml_with_id(bin_path: &Path, service_id: &str) -> String {
    // Placeholder ports; daemon supervisor will override with assigned port at start
    let placeholder_port = 1u16;
    format!(
        r#"
[[services]]
id = "{id}"
name = "E2E HTTP"
command = "{bin}"
restartPolicy = "never"
gracefulTimeoutSecs = 5
startupTimeoutSecs = 20

[services.readinessCheck]
initialDelaySecs = 0
intervalSecs = 1
timeoutSecs = 2
successThreshold = 1

[services.readinessCheck.checkType]
type = "tcp"
port = {ph}

[services.healthCheck]
intervalSecs = 1
timeoutSecs = 2
failureThreshold = 3
successThreshold = 1

[services.healthCheck.checkType]
type = "tcp"
port = {ph}
"#,
        id = service_id,
        bin = bin_path.display(),
        ph = placeholder_port,
    )
}

async fn wait_until_ready(
    client: &JsonRpcClient,
    service_id: &str,
    timeout_ms: u64,
) -> ipc::Result<()> {
    use tokio::time::{sleep, Duration, Instant};
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match client.list().await {
            Ok(services) => {
                if let Some(s) = services.into_iter().find(|s| s.id == service_id) {
                    if s.state == schema::ServiceState::Ready {
                        return Ok(());
                    }
                }
            }
            Err(_) => {
                // Server might not be accepting yet; retry until deadline
            }
        }
        if Instant::now() >= deadline {
            return Err(ipc::IpcError::ProtocolError("ready wait timed out".into()));
        }
        sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
async fn http_start_with_port_and_hostname_and_list_status_show_them() {
    // Temp workspace
    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().to_path_buf();

    // Resolve e2e_http binary and choose service id
    let bin_path = e2e_http_bin_path();
    let service_id = "e2e-http-1";

    // UDS socket (short path in /tmp) and state file (in temp dir)
    let sock_path = uds_socket_path(service_id);
    let state_path = base.join("state.json");
    std::env::set_var("CANOPUS_IPC_SOCKET", &sock_path);
    std::env::set_var("CANOPUS_STATE_FILE", &state_path);

    // Choose a free port
    let allocator = canopus_core::PortAllocator::new();
    let guard = allocator.reserve(None).expect("reserve port");
    let port = guard.port();
    drop(guard);

    // Write services TOML
    let cfg_path = base.join("services.toml");
    std::fs::write(&cfg_path, make_services_toml_with_id(&bin_path, service_id))
        .expect("write services.toml");

    // Bootstrap daemon
    let boot = daemon::bootstrap::bootstrap(Some(cfg_path.clone()))
        .await
        .expect("bootstrap ok");

    // Wait for socket to appear
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if sock_path.exists() {
                break;
            }
            if Instant::now() >= deadline {
                panic!("IPC socket not created in time");
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // Client
    let client = JsonRpcClient::new(&sock_path, None);

    // Retry list to ensure server is up
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if client.list().await.is_ok() {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // Start with explicit port and hostname
    let hostname = "e2e.dev".to_string();
    client
        .start(service_id, Some(port), Some(hostname.as_str()))
        .await
        .expect("start ok");

    // Wait for Ready
    wait_until_ready(&client, service_id, 10_000)
        .await
        .expect("ready");

    // Verify list contains port and hostname (allow brief propagation delay)
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let services = client.list().await.expect("list ok");
            if let Some(svc) = services.into_iter().find(|s| s.id == service_id) {
                if svc.port == Some(port) && svc.hostname.as_deref() == Some(hostname.as_str()) {
                    break;
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    }
    let services = client.list().await.expect("list ok");
    let svc = services
        .into_iter()
        .find(|s| s.id == service_id)
        .expect("svc present");
    assert_eq!(svc.port, Some(port));
    assert_eq!(svc.hostname.as_deref(), Some(hostname.as_str()));

    // Verify status also contains them
    let st = client.status(service_id).await.expect("status ok");
    assert_eq!(st.port, Some(port));
    assert_eq!(st.hostname.as_deref(), Some(hostname.as_str()));

    // Quick health check call
    let healthy = client.health_check(service_id).await.expect("health ok");
    assert!(healthy);

    // Shutdown daemon to avoid interference with subsequent tests
    boot.shutdown().await;
}

#[tokio::test]
async fn http_start_without_port_allocator_assigns_and_list_status_show_it() {
    // Temp workspace
    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().to_path_buf();

    // Resolve e2e_http binary and choose service id
    let bin_path = e2e_http_bin_path();
    let service_id = "e2e-http-2";

    // UDS socket (short path in /tmp) and state file (in temp dir)
    let sock_path = uds_socket_path(service_id);
    let state_path = base.join("state.json");
    std::env::set_var("CANOPUS_IPC_SOCKET", &sock_path);
    std::env::set_var("CANOPUS_STATE_FILE", &state_path);

    // Write services TOML
    let cfg_path = base.join("services.toml");
    std::fs::write(&cfg_path, make_services_toml_with_id(&bin_path, service_id))
        .expect("write services.toml");

    // Bootstrap daemon
    let boot = daemon::bootstrap::bootstrap(Some(cfg_path.clone()))
        .await
        .expect("bootstrap ok");

    // Wait for socket to appear
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if sock_path.exists() {
                break;
            }
            if Instant::now() >= deadline {
                panic!("IPC socket not created in time");
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // Client
    let client = JsonRpcClient::new(&sock_path, None);

    // Retry list to ensure server is up
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if client.list().await.is_ok() {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // Start without explicit port or hostname (allocator should assign a free port)
    client
        .start(service_id, None, None)
        .await
        .expect("start ok");

    // Wait for Ready
    wait_until_ready(&client, service_id, 10_000)
        .await
        .expect("ready");

    // Verify list shows a non-zero port
    let services = client.list().await.expect("list ok");
    let svc = services
        .into_iter()
        .find(|s| s.id == service_id)
        .expect("svc present");
    let port = svc.port.expect("allocator should assign a port");
    assert!(port > 0);

    // Verify status reports the same port
    let st = client.status(service_id).await.expect("status ok");
    assert_eq!(st.port, Some(port));

    // Health should be true
    let healthy = client.health_check(service_id).await.expect("health ok");
    assert!(healthy);

    // Shutdown daemon to avoid interference with subsequent tests
    boot.shutdown().await;
}

#[tokio::test]
async fn http_restart_keeps_port_and_hostname_and_ready_again() {
    // Temp workspace
    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().to_path_buf();

    // Resolve e2e_http binary and choose service id
    let bin_path = e2e_http_bin_path();
    let service_id = "e2e-http-4";

    // UDS socket (short path in /tmp) and state file (in temp dir)
    let sock_path = uds_socket_path(service_id);
    let state_path = base.join("state.json");
    std::env::set_var("CANOPUS_IPC_SOCKET", &sock_path);
    std::env::set_var("CANOPUS_STATE_FILE", &state_path);

    // Choose a free port
    let allocator = canopus_core::PortAllocator::new();
    let guard = allocator.reserve(None).expect("reserve port");
    let port = guard.port();
    drop(guard);

    // Write services TOML
    let cfg_path = base.join("services.toml");
    std::fs::write(&cfg_path, make_services_toml_with_id(&bin_path, service_id))
        .expect("write services.toml");

    // Bootstrap daemon
    let boot = daemon::bootstrap::bootstrap(Some(cfg_path.clone()))
        .await
        .expect("bootstrap ok");

    // Wait for socket to appear
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if sock_path.exists() {
                break;
            }
            if Instant::now() >= deadline {
                panic!("IPC socket not created in time");
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // Client
    let client = JsonRpcClient::new(&sock_path, None);

    // Retry list to ensure server is up
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if client.list().await.is_ok() {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // Start with explicit port and hostname
    let hostname = "e2e-restart.dev".to_string();
    client
        .start(service_id, Some(port), Some(hostname.as_str()))
        .await
        .expect("start ok");

    // Wait for Ready
    wait_until_ready(&client, service_id, 10_000)
        .await
        .expect("ready");

    // Verify initial list/status
    let services = client.list().await.expect("list ok");
    let svc = services
        .into_iter()
        .find(|s| s.id == service_id)
        .expect("svc present");
    assert_eq!(svc.port, Some(port));
    assert_eq!(svc.hostname.as_deref(), Some(hostname.as_str()));
    let st = client.status(service_id).await.expect("status ok");
    assert_eq!(st.port, Some(port));
    assert_eq!(st.hostname.as_deref(), Some(hostname.as_str()));

    // Restart
    client.restart(service_id).await.expect("restart ok");

    // Wait for Ready again
    wait_until_ready(&client, service_id, 10_000)
        .await
        .expect("ready after restart");

    // Verify port and hostname are unchanged after restart
    let services = client.list().await.expect("list ok");
    let svc = services
        .into_iter()
        .find(|s| s.id == service_id)
        .expect("svc present");
    assert_eq!(svc.port, Some(port));
    assert_eq!(svc.hostname.as_deref(), Some(hostname.as_str()));
    let st = client.status(service_id).await.expect("status ok");
    assert_eq!(st.port, Some(port));
    assert_eq!(st.hostname.as_deref(), Some(hostname.as_str()));

    // Health should still be true
    let healthy = client.health_check(service_id).await.expect("health ok");
    assert!(healthy);

    // Shutdown daemon to avoid interference with subsequent tests
    boot.shutdown().await;
}

#[tokio::test]
async fn http_duplicate_start_is_idempotent_and_keeps_settings() {
    // Temp workspace
    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().to_path_buf();

    // Resolve e2e_http binary and choose service id
    let bin_path = e2e_http_bin_path();
    let service_id = "e2e-http-5";

    // UDS socket (short path in /tmp) and state file (in temp dir)
    let sock_path = uds_socket_path(service_id);
    let state_path = base.join("state.json");
    std::env::set_var("CANOPUS_IPC_SOCKET", &sock_path);
    std::env::set_var("CANOPUS_STATE_FILE", &state_path);

    // Choose a free port
    let allocator = canopus_core::PortAllocator::new();
    let guard = allocator.reserve(None).expect("reserve port");
    let port = guard.port();
    drop(guard);

    // Write services TOML
    let cfg_path = base.join("services.toml");
    std::fs::write(&cfg_path, make_services_toml_with_id(&bin_path, service_id))
        .expect("write services.toml");

    // Bootstrap daemon
    let boot = daemon::bootstrap::bootstrap(Some(cfg_path.clone()))
        .await
        .expect("bootstrap ok");

    // Wait for socket to appear
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if sock_path.exists() {
                break;
            }
            if Instant::now() >= deadline {
                panic!("IPC socket not created in time");
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // Client
    let client = JsonRpcClient::new(&sock_path, None);

    // Retry list to ensure server is up
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if client.list().await.is_ok() {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // Start with explicit port and hostname
    let hostname = "e2e-dup-start.dev".to_string();
    client
        .start(service_id, Some(port), Some(hostname.as_str()))
        .await
        .expect("start ok");

    // Wait for Ready
    wait_until_ready(&client, service_id, 10_000)
        .await
        .expect("ready");

    // Start again (idempotent)
    client
        .start(service_id, Some(port), Some(hostname.as_str()))
        .await
        .expect("duplicate start ok");

    // Still Ready and settings preserved
    wait_until_ready(&client, service_id, 10_000)
        .await
        .expect("still ready");
    let services = client.list().await.expect("list ok");
    let svc = services
        .into_iter()
        .find(|s| s.id == service_id)
        .expect("svc present");
    assert_eq!(svc.port, Some(port));
    assert_eq!(svc.hostname.as_deref(), Some(hostname.as_str()));
    let st = client.status(service_id).await.expect("status ok");
    assert_eq!(st.port, Some(port));
    assert_eq!(st.hostname.as_deref(), Some(hostname.as_str()));

    // Shutdown daemon to avoid interference with subsequent tests
    boot.shutdown().await;
}

#[tokio::test]
async fn http_start_with_hostname_only_and_list_status_show_port_and_hostname() {
    // Temp workspace
    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().to_path_buf();

    // Resolve e2e_http binary and choose service id
    let bin_path = e2e_http_bin_path();
    let service_id = "e2e-http-3";

    // UDS socket (short path in /tmp) and state file (in temp dir)
    let sock_path = uds_socket_path(service_id);
    let state_path = base.join("state.json");
    std::env::set_var("CANOPUS_IPC_SOCKET", &sock_path);
    std::env::set_var("CANOPUS_STATE_FILE", &state_path);

    // Write services TOML
    let cfg_path = base.join("services.toml");
    std::fs::write(&cfg_path, make_services_toml_with_id(&bin_path, service_id))
        .expect("write services.toml");

    // Bootstrap daemon
    let boot = daemon::bootstrap::bootstrap(Some(cfg_path.clone()))
        .await
        .expect("bootstrap ok");

    // Wait for socket to appear
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if sock_path.exists() {
                break;
            }
            if Instant::now() >= deadline {
                panic!("IPC socket not created in time");
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // Client
    let client = JsonRpcClient::new(&sock_path, None);

    // Retry list to ensure server is up
    {
        use tokio::time::{sleep, Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if client.list().await.is_ok() {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // Start with hostname only (port should be assigned automatically)
    let hostname = "e2e-only-host.dev";
    client
        .start(service_id, None, Some(hostname))
        .await
        .expect("start ok");

    // Wait for Ready
    wait_until_ready(&client, service_id, 10_000)
        .await
        .expect("ready");

    // Verify list contains a port and the hostname
    let services = client.list().await.expect("list ok");
    let svc = services
        .into_iter()
        .find(|s| s.id == service_id)
        .expect("svc present");
    let port = svc.port.expect("allocator should assign a port");
    assert!(port > 0);
    assert_eq!(svc.hostname.as_deref(), Some(hostname));

    // Verify status also contains them
    let st = client.status(service_id).await.expect("status ok");
    assert_eq!(st.port, Some(port));
    assert_eq!(st.hostname.as_deref(), Some(hostname));

    // Quick health check call
    let healthy = client.health_check(service_id).await.expect("health ok");
    assert!(healthy);

    // Shutdown daemon to avoid interference with subsequent tests
    boot.shutdown().await;
}
