//! CLI library for the Canopus project

#![allow(unused_crate_dependencies)]

pub mod error;

pub use error::{CliError, Result};
use ipc::IpcClient;
use schema::{ClientConfig, Message, Response};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tokio::time::{sleep, Duration};
use tracing::error;

use canopus_core::config::{load_simple_services_from_toml_path, SimpleServicesFile};
use ipc::uds_client::JsonRpcClient;

/// CLI client for communicating with the daemon
#[derive(Debug)]
pub struct Client {
    #[allow(dead_code)]
    config: ClientConfig,
    ipc_client: IpcClient,
}

impl Client {
    /// Create a new CLI client
    #[must_use]
    pub fn new(config: ClientConfig) -> Self {
        let ipc_client = IpcClient::new(&config.daemon_host, config.daemon_port);
        Self { config, ipc_client }
    }

    /// Start the daemon and, if a config is provided, synchronize services to match it
    ///
    /// # Errors
    /// Returns an error if the daemon cannot be reached or the config cannot be applied.
    pub async fn start_with_config(&self, config: Option<PathBuf>) -> Result<()> {
        // Ensure daemon is running (reuses existing logic)
        self.start().await?;

        // If no config provided, nothing more to do
        let Some(cfg_path) = config else {
            return Ok(());
        };
        // Apply the runtime config
        return self.apply_runtime_config_path(&cfg_path).await;
    }

    /// Connect to the daemon and send a message
    ///
    /// # Errors
    /// Returns an error if the daemon cannot be reached or returns an IPC error.
    pub async fn send_message(&self, message: Message) -> Result<Response> {
        self.ipc_client
            .send_message(&message)
            .await
            .map_err(CliError::IpcError)
    }

    /// Get daemon status
    ///
    /// # Errors
    /// Returns an error if the daemon cannot be reached or returns an error response.
    pub async fn status(&self) -> Result<()> {
        match self.send_message(Message::Status).await {
            Ok(Response::Status {
                running,
                uptime_seconds,
                pid,
                version,
            }) => {
                println!("Daemon Status:");
                println!("  Running: {running}");
                println!("  Uptime: {uptime_seconds} seconds");
                println!("  PID: {pid}");
                if let Some(v) = version {
                    println!("  Version: {v}");
                }
            }
            Ok(Response::Error { message, code }) => {
                error!("Error getting status: {}", message);
                if let Some(c) = code {
                    error!("Error code: {}", c);
                }
                return Err(CliError::DaemonError(message));
            }
            Ok(Response::Ok { .. }) => {
                let err = "Unexpected response type".to_string();
                error!("{}", err);
                return Err(CliError::DaemonError(err));
            }
            Err(CliError::IpcError(ipc_err)) => {
                // If the daemon isn't running, report a friendly status instead of a raw connection error
                if let ipc::IpcError::ConnectionFailed(_) = ipc_err {
                    println!("Daemon Status:");
                    println!("  Running: false");
                    println!("  Note: daemon not started");
                    return Ok(());
                }
                return Err(CliError::IpcError(ipc_err));
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }

    /// Start the daemon
    ///
    /// # Errors
    /// Returns an error if the daemon cannot be reached or rejects the request.
    pub async fn start(&self) -> Result<()> {
        // First, try to send Start to a running daemon
        match self.send_message(Message::Start).await {
            Ok(resp) => match resp {
                Response::Ok { message } => {
                    println!("✓ {message}");
                    Ok(())
                }
                Response::Error { message, code } => {
                    // Treat already running as success
                    if matches!(code.as_deref(), Some("DAEMON_ALREADY_RUNNING")) {
                        println!("✓ Daemon already running");
                        Ok(())
                    } else {
                        error!("Failed to start daemon: {}", message);
                        if let Some(c) = code {
                            error!("Error code: {}", c);
                        }
                        Err(CliError::DaemonError(message))
                    }
                }
                Response::Status { .. } => {
                    let err = "Unexpected response type".to_string();
                    error!("{}", err);
                    Err(CliError::DaemonError(err))
                }
            },
            Err(CliError::IpcError(ipc_err)) => {
                // Connection failure: try to spawn the daemon, then wait for readiness
                if let ipc::IpcError::ConnectionFailed(_) = ipc_err {
                    println!("Daemon not running; attempting to start it...");
                    self.spawn_daemon_background()?;
                    // Wait up to 20s for the daemon to become reachable
                    self.wait_for_ready(Duration::from_secs(20)).await?;
                    // After ready, sending Start is optional; do it for symmetry
                    match self.send_message(Message::Start).await {
                        Ok(Response::Ok { message }) => {
                            println!("✓ {message}");
                            Ok(())
                        }
                        Ok(Response::Error { message, code }) => {
                            if matches!(code.as_deref(), Some("DAEMON_ALREADY_RUNNING")) {
                                println!("✓ Daemon started and is running");
                                Ok(())
                            } else {
                                error!("Failed to start daemon: {}", message);
                                if let Some(c) = code {
                                    error!("Error code: {}", c);
                                }
                                Err(CliError::DaemonError(message))
                            }
                        }
                        Ok(Response::Status { .. }) => {
                            let err = "Unexpected response type".to_string();
                            error!("{}", err);
                            Err(CliError::DaemonError(err))
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Err(CliError::IpcError(ipc_err))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Stop the daemon
    ///
    /// # Errors
    /// Returns an error if the daemon cannot be reached or rejects the request.
    pub async fn stop(&self) -> Result<()> {
        match self.send_message(Message::Stop).await? {
            Response::Ok { message } => {
                println!("✓ {message}");
            }
            Response::Error { message, code } => {
                error!("Failed to stop daemon: {}", message);
                if let Some(c) = code {
                    error!("Error code: {}", c);
                }
                return Err(CliError::DaemonError(message));
            }
            Response::Status { .. } => {
                let err = "Unexpected response type".to_string();
                error!("{}", err);
                return Err(CliError::DaemonError(err));
            }
        }
        Ok(())
    }

    /// Restart the daemon
    ///
    /// # Errors
    /// Returns an error if the daemon cannot be reached or rejects the request.
    pub async fn restart(&self) -> Result<()> {
        match self.send_message(Message::Restart).await? {
            Response::Ok { message } => {
                println!("✓ {message}");
            }
            Response::Error { message, code } => {
                error!("Failed to restart daemon: {}", message);
                if let Some(c) = code {
                    error!("Error code: {}", c);
                }
                return Err(CliError::DaemonError(message));
            }
            Response::Status { .. } => {
                let err = "Unexpected response type".to_string();
                error!("{}", err);
                return Err(CliError::DaemonError(err));
            }
        }
        Ok(())
    }

    /// Send a custom command
    ///
    /// # Errors
    /// Returns an error if the daemon cannot be reached or rejects the request.
    pub async fn custom(&self, command: &str) -> Result<()> {
        match self
            .send_message(Message::Custom {
                cmd: command.to_string(),
            })
            .await?
        {
            Response::Ok { message } => {
                println!("✓ {message}");
            }
            Response::Error { message, code } => {
                error!("Command failed: {}", message);
                if let Some(c) = code {
                    error!("Error code: {}", c);
                }
                return Err(CliError::DaemonError(message));
            }
            Response::Status { .. } => {
                let err = "Unexpected response type".to_string();
                error!("{}", err);
                return Err(CliError::DaemonError(err));
            }
        }
        Ok(())
    }
}

impl Client {
    /// Apply a simple runtime config (hostname/port per service) from a file path
    ///
    /// # Errors
    /// Returns an error if the runtime config cannot be parsed or applied.
    pub async fn apply_runtime_config_path(&self, cfg_path: &PathBuf) -> Result<()> {
        // Parse simple config: tables per service id with optional hostname/port
        let simple: SimpleServicesFile = load_simple_services_from_toml_path(cfg_path)
            .map_err(|e| CliError::DaemonError(format!("config error: {e}")))?;

        // Connect to local UDS control plane
        let socket =
            std::env::var("CANOPUS_IPC_SOCKET").unwrap_or_else(|_| "/tmp/canopus.sock".to_string());
        let uds = JsonRpcClient::new(socket, std::env::var("CANOPUS_IPC_TOKEN").ok());

        // Gather current services from daemon
        let current = uds.list().await.map_err(CliError::IpcError)?;
        let current_ids: HashSet<String> = current.iter().map(|s| s.id.clone()).collect();

        // Compute desired set from config
        let desired_ids: HashSet<String> = simple.services.keys().cloned().collect();

        // Stop and delete services not in desired set
        for s in &current {
            if !desired_ids.contains(&s.id) {
                // Best-effort stop
                let _ = uds.stop(&s.id).await;
                // Remove metadata row from SQLite
                let _ = uds.delete_meta(&s.id).await;
                println!("Removed service '{}' from DB (not in config)", s.id);
            }
        }

        // For each desired service, start if idle; skip if already running
        for (id, cfg) in &simple.services {
            if !current_ids.contains(id) {
                // Unknown service id to the daemon; skip with a warning
                println!(
                    "Warning: service '{id}' not found in daemon; ensure it is defined in daemon's services config"
                );
                continue;
            }

            let detail = uds.status(id).await.map_err(CliError::IpcError)?;
            if detail.state != schema::ServiceState::Idle {
                println!("Skipping '{}': already running ({:?})", id, detail.state);
                continue;
            }

            uds.start(id, cfg.port, cfg.hostname.as_deref())
                .await
                .map_err(CliError::IpcError)?;
            println!(
                "Started '{}'{}{}",
                id,
                cfg.port
                    .map(|p| format!(" on port {p}"))
                    .unwrap_or_default(),
                cfg.hostname
                    .as_ref()
                    .map(|h| format!(" with host {h}"))
                    .unwrap_or_default()
            );
        }

        Ok(())
    }
}

impl Client {
    /// Start the daemon, optionally passing a full services config to the daemon when spawning,
    /// and then apply a simple runtime config (hostname/port) via UDS.
    ///
    /// # Errors
    /// Returns an error if the daemon cannot be reached, started, or configured.
    pub async fn start_with_configs(
        &self,
        services_config: Option<PathBuf>,
        runtime_config: Option<PathBuf>,
    ) -> Result<()> {
        // First try to talk to a running daemon
        let mut need_spawn = false;
        match self.send_message(Message::Status).await {
            Ok(Response::Status { .. }) => {
                // Daemon is reachable; continue
            }
            Ok(_) => {
                need_spawn = true;
            }
            Err(CliError::IpcError(ipc_err)) => {
                if let ipc::IpcError::ConnectionFailed(_) = ipc_err {
                    need_spawn = true;
                } else {
                    return Err(CliError::IpcError(ipc_err));
                }
            }
            Err(e) => return Err(e),
        }

        if need_spawn {
            println!("Daemon not running; attempting to start it...");
            self.spawn_daemon_background_with_configs(
                services_config.as_ref(),
                runtime_config.as_ref(),
            )?;
            // Wait up to 20s for the daemon to become reachable
            self.wait_for_ready(Duration::from_secs(20)).await?;
            // Send Start for symmetry
            match self.send_message(Message::Start).await {
                Ok(Response::Ok { message }) => {
                    println!("✓ {message}");
                }
                Ok(Response::Error { message, code }) => {
                    if !matches!(code.as_deref(), Some("DAEMON_ALREADY_RUNNING")) {
                        error!("Failed to start daemon: {}", message);
                        if let Some(c) = code {
                            error!("Error code: {}", c);
                        }
                        return Err(CliError::DaemonError(message));
                    }
                }
                Ok(_) => {
                    let err = "Unexpected response type".to_string();
                    error!("{}", err);
                    return Err(CliError::DaemonError(err));
                }
                Err(e) => return Err(e),
            }
        }

        // Apply runtime config if provided
        if let Some(rt) = runtime_config.as_ref() {
            self.apply_runtime_config_path(rt).await?;
        }

        Ok(())
    }
}

impl Client {
    /// Get daemon semantic version via TCP Status response
    ///
    /// # Errors
    /// Returns an error if the daemon cannot be reached or does not report a version.
    pub async fn version(&self) -> Result<()> {
        match self.send_message(Message::Status).await? {
            Response::Status { version, .. } => version.map_or_else(
                || Err(CliError::DaemonError("version unavailable".into())),
                |v| {
                    println!("{v}");
                    Ok(())
                },
            ),
            Response::Error { message, .. } => Err(CliError::DaemonError(message)),
            Response::Ok { .. } => Err(CliError::DaemonError("Unexpected response".into())),
        }
    }
}

impl Client {
    async fn wait_for_ready(&self, timeout: Duration) -> Result<()> {
        let start = std::time::Instant::now();
        let mut last_err: Option<CliError> = None;
        while start.elapsed() < timeout {
            match self.send_message(Message::Status).await {
                Ok(Response::Status { running, .. }) => {
                    if running {
                        return Ok(());
                    }
                }
                Ok(_) => { /* keep trying */ }
                Err(e) => {
                    last_err = Some(e);
                }
            }
            sleep(Duration::from_millis(200)).await;
        }
        Err(last_err
            .unwrap_or_else(|| CliError::ConnectionFailed("Timed out waiting for daemon".into())))
    }

    fn spawn_daemon_background(&self) -> Result<()> {
        // Prefer spawning the daemon binary from the same target directory as this CLI binary
        if let Ok(path) = std::env::current_exe() {
            if let Some(bin_dir) = path.parent() {
                // Replace canopus with daemon, preserving .exe suffix on Windows if any
                let file_name = if cfg!(windows) {
                    "daemon.exe"
                } else {
                    "daemon"
                };
                let daemon_path: PathBuf = bin_dir.join(file_name);
                if daemon_path.exists() {
                    let _child = Command::new(&daemon_path)
                        .arg("--host")
                        .arg(&self.config.daemon_host)
                        .arg("--port")
                        .arg(self.config.daemon_port.to_string())
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .map_err(CliError::IoError)?;
                    return Ok(());
                }
            }
        }

        // Fallback: rely on PATH to find a `daemon` binary
        let _child = Command::new("daemon")
            .arg("--host")
            .arg(&self.config.daemon_host)
            .arg("--port")
            .arg(self.config.daemon_port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(CliError::IoError)?;
        Ok(())
    }

    /// Spawn the daemon in the background, optionally passing a services config and runtime config.
    fn spawn_daemon_background_with_configs(
        &self,
        services_config: Option<&PathBuf>,
        runtime_config: Option<&PathBuf>,
    ) -> Result<()> {
        // Helper to append common args and optional configs
        fn build_cmd<'a>(
            mut cmd: Command,
            host: &str,
            port: u16,
            services_config: Option<&'a PathBuf>,
            runtime_config: Option<&'a PathBuf>,
        ) -> Command {
            cmd.arg("--host")
                .arg(host)
                .arg("--port")
                .arg(port.to_string());
            if let Some(p) = services_config {
                cmd.arg("--config").arg(p);
            }
            if let Some(p) = runtime_config {
                cmd.arg("--runtime-config").arg(p);
            }
            cmd.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            cmd
        }

        // Prefer spawning the daemon binary from the same target directory as this CLI binary
        if let Ok(path) = std::env::current_exe() {
            if let Some(bin_dir) = path.parent() {
                let file_name = if cfg!(windows) {
                    "daemon.exe"
                } else {
                    "daemon"
                };
                let daemon_path: PathBuf = bin_dir.join(file_name);
                if daemon_path.exists() {
                    let mut cmd = build_cmd(
                        Command::new(&daemon_path),
                        &self.config.daemon_host,
                        self.config.daemon_port,
                        services_config,
                        runtime_config,
                    );
                    let _child = cmd.spawn().map_err(CliError::IoError)?;
                    return Ok(());
                }
            }
        }

        // Fallback: rely on PATH to find a `daemon` binary
        let mut cmd = build_cmd(
            Command::new("daemon"),
            &self.config.daemon_host,
            self.config.daemon_port,
            services_config,
            runtime_config,
        );
        let _child = cmd.spawn().map_err(CliError::IoError)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    /// Create a Client pointing to a specific local port.
    fn make_client(port: u16) -> Client {
        Client::new(ClientConfig {
            daemon_host: "127.0.0.1".to_string(),
            daemon_port: port,
            timeout_seconds: 5,
        })
    }

    /// Spawn a one-shot mock TCP server that reads one newline-terminated request,
    /// validates it deserializes as a `Message`, then writes `response_json`
    /// followed by `\n`. Returns the bound port and the server `JoinHandle` so
    /// callers can await it and surface any panics that occur inside the task.
    async fn mock_server_once(response_json: &'static str) -> (u16, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            let _ = reader.read_line(&mut line).await;
            // Validate the client sent a well-formed Message; a panic here will
            // propagate when the test awaits the handle.
            serde_json::from_str::<Message>(line.trim_end())
                .expect("mock server: client sent invalid Message JSON");
            let mut resp = response_json.as_bytes().to_vec();
            resp.push(b'\n');
            writer.write_all(&resp).await.unwrap();
        });
        (port, handle)
    }

    // ── Client::new ───────────────────────────────────────────────────────────

    #[test]
    fn client_new_stores_config() {
        let config = ClientConfig {
            daemon_host: "10.0.0.1".to_string(),
            daemon_port: 9999,
            timeout_seconds: 15,
        };
        let client = Client::new(config);
        assert_eq!(client.config.daemon_host, "10.0.0.1");
        assert_eq!(client.config.daemon_port, 9999);
        assert_eq!(client.config.timeout_seconds, 15);
    }

    // ── status() ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn status_returns_ok_when_daemon_running() {
        let (port, server) =
            mock_server_once(r#"{"status":{"running":true,"uptime_seconds":42,"pid":999}}"#).await;
        let client = make_client(port);
        let result = client.status().await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn status_returns_ok_when_daemon_running_false() {
        let (port, server) =
            mock_server_once(r#"{"status":{"running":false,"uptime_seconds":0,"pid":0}}"#).await;
        let client = make_client(port);
        let result = client.status().await;
        assert!(
            result.is_ok(),
            "expected Ok even for running=false, got {result:?}"
        );
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn status_returns_ok_when_connection_refused() {
        // Port 1 is a privileged port never bound in test environments, so
        // connecting to it deterministically yields IpcError::ConnectionFailed.
        let client = make_client(1);
        let result = client.status().await;
        // ConnectionFailed is treated as "not started" → still Ok
        assert!(
            result.is_ok(),
            "expected Ok for connection refused, got {result:?}"
        );
    }

    #[tokio::test]
    async fn status_returns_err_on_error_response() {
        let (port, server) = mock_server_once(r#"{"error":{"message":"internal error"}}"#).await;
        let client = make_client(port);
        let result = client.status().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CliError::DaemonError(_)));
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn status_returns_err_on_unexpected_ok_response() {
        let (port, server) = mock_server_once(r#"{"ok":{"message":"unexpected"}}"#).await;
        let client = make_client(port);
        let result = client.status().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CliError::DaemonError(_)));
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn status_includes_version_when_present() {
        let (port, server) = mock_server_once(
            r#"{"status":{"running":true,"uptime_seconds":0,"pid":1,"version":"2.0.0"}}"#,
        )
        .await;
        let client = make_client(port);
        let result = client.status().await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        server.await.expect("mock server panicked");
    }

    // ── stop() ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn stop_returns_ok_on_success() {
        let (port, server) = mock_server_once(r#"{"ok":{"message":"Daemon stopped"}}"#).await;
        let client = make_client(port);
        let result = client.stop().await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn stop_returns_err_on_error_response() {
        let (port, server) =
            mock_server_once(r#"{"error":{"message":"stop failed","code":"STOP001"}}"#).await;
        let client = make_client(port);
        let result = client.stop().await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CliError::DaemonError(_)));
        assert!(err.to_string().contains("stop failed"));
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn stop_returns_err_on_unexpected_status_response() {
        let (port, server) =
            mock_server_once(r#"{"status":{"running":true,"uptime_seconds":0,"pid":1}}"#).await;
        let client = make_client(port);
        let result = client.stop().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CliError::DaemonError(_)));
        server.await.expect("mock server panicked");
    }

    // ── restart() ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn restart_returns_ok_on_success() {
        let (port, server) = mock_server_once(r#"{"ok":{"message":"Daemon restarted"}}"#).await;
        let client = make_client(port);
        let result = client.restart().await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn restart_returns_err_on_error_response() {
        let (port, server) = mock_server_once(r#"{"error":{"message":"restart failed"}}"#).await;
        let client = make_client(port);
        let result = client.restart().await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CliError::DaemonError(_)));
        assert!(err.to_string().contains("restart failed"));
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn restart_returns_err_on_unexpected_status_response() {
        let (port, server) =
            mock_server_once(r#"{"status":{"running":true,"uptime_seconds":0,"pid":1}}"#).await;
        let client = make_client(port);
        let result = client.restart().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CliError::DaemonError(_)));
        server.await.expect("mock server panicked");
    }

    // ── custom() ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn custom_returns_ok_on_success() {
        let (port, server) = mock_server_once(r#"{"ok":{"message":"command executed"}}"#).await;
        let client = make_client(port);
        let result = client.custom("my-command").await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn custom_returns_err_on_error_response() {
        let (port, server) = mock_server_once(r#"{"error":{"message":"command failed"}}"#).await;
        let client = make_client(port);
        let result = client.custom("my-command").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CliError::DaemonError(_)));
        assert!(err.to_string().contains("command failed"));
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn custom_returns_err_on_unexpected_status_response() {
        let (port, server) =
            mock_server_once(r#"{"status":{"running":true,"uptime_seconds":0,"pid":1}}"#).await;
        let client = make_client(port);
        let result = client.custom("anything").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CliError::DaemonError(_)));
        server.await.expect("mock server panicked");
    }

    // ── version() ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn version_prints_version_when_present() {
        let (port, server) = mock_server_once(
            r#"{"status":{"running":true,"uptime_seconds":0,"pid":1,"version":"1.2.3"}}"#,
        )
        .await;
        let client = make_client(port);
        let result = client.version().await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn version_returns_err_when_version_absent() {
        let (port, server) =
            mock_server_once(r#"{"status":{"running":true,"uptime_seconds":0,"pid":1}}"#).await;
        let client = make_client(port);
        let result = client.version().await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CliError::DaemonError(_)));
        assert!(err.to_string().contains("unavailable"));
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn version_returns_err_on_error_response() {
        let (port, server) = mock_server_once(r#"{"error":{"message":"version error"}}"#).await;
        let client = make_client(port);
        let result = client.version().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CliError::DaemonError(_)));
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn version_returns_err_on_unexpected_ok_response() {
        let (port, server) = mock_server_once(r#"{"ok":{"message":"unexpected"}}"#).await;
        let client = make_client(port);
        let result = client.version().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CliError::DaemonError(_)));
        server.await.expect("mock server panicked");
    }

    // ── start() special cases ─────────────────────────────────────────────────

    #[tokio::test]
    async fn start_returns_ok_on_daemon_already_running() {
        let (port, server) = mock_server_once(
            r#"{"error":{"message":"already running","code":"DAEMON_ALREADY_RUNNING"}}"#,
        )
        .await;
        let client = make_client(port);
        let result = client.start().await;
        // DAEMON_ALREADY_RUNNING error code is treated as success
        assert!(
            result.is_ok(),
            "expected Ok for DAEMON_ALREADY_RUNNING, got {result:?}"
        );
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn start_returns_ok_on_success_ok_response() {
        let (port, server) = mock_server_once(r#"{"ok":{"message":"daemon started"}}"#).await;
        let client = make_client(port);
        let result = client.start().await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn start_returns_err_on_error_response_with_unknown_code() {
        let (port, server) =
            mock_server_once(r#"{"error":{"message":"start failed","code":"SOME_ERROR"}}"#).await;
        let client = make_client(port);
        let result = client.start().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CliError::DaemonError(_)));
        server.await.expect("mock server panicked");
    }

    #[tokio::test]
    async fn start_returns_err_on_unexpected_status_response() {
        let (port, server) =
            mock_server_once(r#"{"status":{"running":true,"uptime_seconds":0,"pid":1}}"#).await;
        let client = make_client(port);
        let result = client.start().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CliError::DaemonError(_)));
        server.await.expect("mock server panicked");
    }

    // ── send_message() ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn send_message_wraps_ipc_connection_failure() {
        // Port 1 is a privileged port never bound in test environments, so
        // connecting to it deterministically yields IpcError::ConnectionFailed.
        let client = make_client(1);
        let result = client.send_message(Message::Status).await;
        assert!(
            matches!(
                result,
                Err(CliError::IpcError(ipc::IpcError::ConnectionFailed(_)))
            ),
            "expected ConnectionFailed IPC error, got {result:?}"
        );
    }

    #[tokio::test]
    async fn send_message_returns_parsed_response() {
        let (port, server) = mock_server_once(r#"{"ok":{"message":"hello from daemon"}}"#).await;
        let client = make_client(port);
        let result = client.send_message(Message::Status).await;
        assert!(result.is_ok());
        assert!(
            matches!(result.unwrap(), Response::Ok { message } if message == "hello from daemon")
        );
        server.await.expect("mock server panicked");
    }
}
