#![allow(unused_crate_dependencies)]
//! End-to-end scenarios: toy TCP service supervised via bootstrap + IPC
//!
//! This test boots the daemon bootstrap (supervisors + UDS IPC), starts a
//! tiny TCP server that prints "ready" then accepts connections, and validates:
//! becoming Ready, tailing logs, restart/stop calls, and snapshot persistence
//! across a simulated restart.

use std::fs;
use std::path::{Path, PathBuf};

use canopus_core::persistence::load_snapshot;
use ipc::uds_client::JsonRpcClient;
pub mod common;

fn toy_bin_path() -> PathBuf {
    // Prefer Cargo-provided binary path
    if let Some(p) = std::env::var_os("CARGO_BIN_EXE_toy_http") {
        return PathBuf::from(p);
    }
    // Fallback: derive from current test exe location (target/debug/deps/...)
    let exe = std::env::current_exe().expect("current_exe");
    let debug_dir = exe.parent().and_then(|p| p.parent()).expect("debug dir");
    let candidate = debug_dir.join("toy_http");
    if candidate.exists() {
        return candidate;
    }
    panic!(
        "Unable to locate toy_http binary; set CARGO_BIN_EXE_toy_http or ensure target/debug/toy_http exists"
    );
}

// removed stray e2e_http helper (moved to e2e-tests crate)

#[allow(dead_code)]
fn make_e2e_http_services_toml(bin_path: &Path) -> String {
    // Placeholder ports; supervisor will override on start
    let placeholder_port = 1u16;
    format!(
        r#"
[[services]]
id = "e2e-http"
name = "E2E HTTP"
command = "{bin}"
restartPolicy = "always"
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
        bin = bin_path.display(),
        ph = placeholder_port,
    )
}

fn make_services_toml(bin_path: &Path, port: u16) -> String {
    format!(
        r#"
[[services]]
id = "toy-http"
name = "Toy HTTP"
command = "{bin}"
args = ["{port}"]
restartPolicy = "always"
gracefulTimeoutSecs = 5
startupTimeoutSecs = 20

[services.environment]
PORT = "{port}"

[services.readinessCheck]
initialDelaySecs = 0
intervalSecs = 1
timeoutSecs = 2
successThreshold = 1

[services.readinessCheck.checkType]
type = "tcp"
port = {port}

[services.healthCheck]
intervalSecs = 1
timeoutSecs = 2
failureThreshold = 3
successThreshold = 1

[services.healthCheck.checkType]
type = "tcp"
port = {port}
"#,
        bin = bin_path.display(),
        port = port,
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
        // Prefer list() for state; fall back to health_check true
        let services = client.list().await?;
        if let Some(s) = services.into_iter().find(|s| s.id == service_id) {
            if s.state == schema::ServiceState::Ready {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(ipc::IpcError::ProtocolError("ready wait timed out".into()));
        }
        sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn e2e_toy_http_flow_ready_logs_restart_stop_persist_recover() {
    common::run_with_timeout(std::time::Duration::from_secs(60), async {
        // Temp workspace for socket/state/config/scripts
        let temp = tempfile::tempdir().expect("tempdir");
        let base = temp.path().to_path_buf();

        // Prepare UDS socket path and state snapshot path
        let sock_path = base.join("canopus.sock");
        let state_path = base.join("state.json");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        std::env::set_var("CANOPUS_IPC_SOCKET", &sock_path);
        std::env::set_var("CANOPUS_STATE_FILE", &state_path);
        std::env::set_var("HOME", &home);

        // Resolve toy HTTP test binary path
        let toy_path = toy_bin_path();

        // Choose a free port using the workspace allocator to avoid collisions
        let allocator = canopus_core::PortAllocator::new();
        let guard = allocator.reserve(None).expect("reserve port");
        let port = guard.port();
        drop(guard); // release before the service starts, minimal race window

        // Write services TOML
        let cfg_path = base.join("services.toml");
        fs::write(&cfg_path, make_services_toml(&toy_path, port)).expect("write services.toml");

        // Start bootstrap (supervisors + IPC server) without binding port 80
        let boot = daemon::bootstrap::bootstrap_with_runtime(Some(cfg_path.clone()), None, None)
            .await
            .expect("bootstrap ok");

        // Wait for IPC socket to be created and accepting
        {
            use tokio::time::{sleep, Duration, Instant};
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if sock_path.exists() {
                    break;
                }
                assert!(Instant::now() < deadline, "IPC socket not created in time");
                sleep(Duration::from_millis(50)).await;
            }
        }

        // Connect IPC client
        let client = JsonRpcClient::new(&sock_path, None);

        // Verify service is listed and initially Idle (retry until server answers)
        let services = {
            use tokio::time::{sleep, Duration, Instant};
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if let Ok(svcs) = client.list().await {
                    break svcs;
                }
                assert!(
                    Instant::now() < deadline,
                    "IPC list did not respond in time"
                );
                sleep(Duration::from_millis(50)).await;
            }
        };
        assert_eq!(services.len(), 1, "one service expected");
        assert_eq!(services[0].id, "toy-http");

        // Start service (no explicit port/hostname)
        client
            .start("toy-http", None, None)
            .await
            .expect("start ok");

        // Tail logs and expect a "ready" line
        let mut log_rx = client.tail_logs("toy-http", None).await.expect("tail ok");

        // Wait for either a ready state or a ready log line
        let mut saw_ready_log = false;
        let ready_wait = wait_until_ready(&client, "toy-http", 20_000);
        tokio::pin!(ready_wait);
        loop {
            tokio::select! {
                maybe_evt = log_rx.recv() => {
                    if let Some(schema::ServiceEvent::LogOutput { content, .. }) = maybe_evt {
                        if content.contains("ready") { saw_ready_log = true; }
                        if saw_ready_log { break; }
                    }
                }
                res = &mut ready_wait => {
                    res.expect("became ready");
                    break;
                }
                () = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }
        }

        // Ensure Ready state before asserting health
        wait_until_ready(&client, "toy-http", 20_000)
            .await
            .expect("became ready");

        // Confirm health check via API returns true
        let healthy = client.health_check("toy-http").await.expect("health ok");
        assert!(healthy, "service should be healthy");

        // Snapshot should exist and indicate Ready with some pid
        let snap = load_snapshot(&state_path).expect("load snapshot");
        let svc = snap
            .services
            .iter()
            .find(|s| s.id == "toy-http")
            .expect("svc in snap");
        assert_eq!(svc.last_state, schema::ServiceState::Ready);
        assert!(svc.last_pid.is_some(), "pid should be recorded");

        // Restart via IPC
        client.restart("toy-http").await.expect("restart ok");
        // Wait back to Ready again
        wait_until_ready(&client, "toy-http", 20_000)
            .await
            .expect("ready after restart");

        // Stop via IPC
        client.stop("toy-http").await.expect("stop ok");

        // Give a moment for snapshot update
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let snap2 = load_snapshot(&state_path).expect("load snapshot2");
        let svc2 = snap2
            .services
            .iter()
            .find(|s| s.id == "toy-http")
            .expect("svc in snap2");
        assert!(matches!(
            svc2.last_state,
            schema::ServiceState::Stopping | schema::ServiceState::Idle
        ));

        // Simulate daemon restart: shutdown bootstrap, then bootstrap again with same config
        boot.shutdown();
        // New bootstrap on same socket/state paths without binding port 80
        let _boot2 = daemon::bootstrap::bootstrap_with_runtime(Some(cfg_path.clone()), None, None)
            .await
            .expect("bootstrap2 ok");

        // Wait for IPC to accept after restart
        {
            use tokio::time::{sleep, Duration, Instant};
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if sock_path.exists() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "IPC socket not recreated in time"
                );
                sleep(Duration::from_millis(50)).await;
            }
        }

        // Auto-start policy Always should start service; wait until Ready again
        let client2 = JsonRpcClient::new(&sock_path, None);
        // retry list before readiness wait
        {
            use tokio::time::{sleep, Duration, Instant};
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if client2.list().await.is_ok() {
                    break;
                }
                if Instant::now() >= deadline {
                    break;
                }
                sleep(Duration::from_millis(50)).await;
            }
        }
        wait_until_ready(&client2, "toy-http", 12_000)
            .await
            .expect("ready after recover");
    })
    .await;
}

// removed e2e_http test (moved to e2e-tests crate)
