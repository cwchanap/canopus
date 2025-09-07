//! Canopus daemon binary
//!
//! The main daemon process that provides system services.

#![allow(unused_crate_dependencies)]

use daemon::Daemon;
use ipc::server::{IpcServer, IpcServerConfig};
use std::sync::Arc;
use canopus_core::supervisor::{spawn_supervisor, SupervisorConfig, UnixProcessAdapter};
use schema::{ServiceSpec, RestartPolicy};
use schema::DaemonConfig;
use tracing::{error, info};

#[tokio::main]
async fn main() -> daemon::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting Canopus Daemon");

    // Load configuration (in a real app, this might come from a config file)
    let config = DaemonConfig::default();

    // Create and start the daemon (TCP prototype server)
    let daemon = Daemon::new(config);

    // Demo: set up a service supervisor and a shared event bus
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(1024);
    let mut handles = std::collections::HashMap::new();

    #[cfg(unix)]
    {
        // Demo service that prints a line periodically
        let spec = ServiceSpec {
            id: "demo-service".to_string(),
            name: "Demo Service".to_string(),
            command: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), "while true; do echo demo-stdout; sleep 1; done".to_string()],
            environment: Default::default(),
            working_directory: None,
            restart_policy: RestartPolicy::Always,
            backoff_config: Default::default(),
            health_check: None,
            readiness_check: None,
            graceful_timeout_secs: 5,
            startup_timeout_secs: 10,
        };
        let process_adapter = Arc::new(UnixProcessAdapter::new());
        let sup_cfg = SupervisorConfig { spec: spec.clone(), process_adapter, event_tx: event_tx.clone() };
        let handle = spawn_supervisor(sup_cfg);
        let _ = handle.start();
        handles.insert(spec.id.clone(), handle);
    }

    // Spawn local IPC server over Unix Domain Socket for JSON-RPC control plane
    #[cfg(unix)]
    {
        let socket_path = std::env::var("CANOPUS_IPC_SOCKET").unwrap_or_else(|_| "/tmp/canopus.sock".to_string());
        let token = std::env::var("CANOPUS_IPC_TOKEN").ok();
        let cfg = IpcServerConfig {
            version: env!("CARGO_PKG_VERSION").to_string(),
            auth_token: token,
            unix_socket_path: Some(std::path::PathBuf::from(socket_path)),
            windows_pipe_name: None,
        };
        let event_tx2 = event_tx.clone();
        let handles2 = handles.clone();
        tokio::spawn(async move {
            let server = {
                let adapter = ipc::server::supervisor_adapter::SupervisorControlPlane::new(handles2, event_tx2);
                IpcServer::with_router(cfg, Arc::new(adapter))
            };
            if let Err(e) = server.serve().await {
                tracing::warn!("IPC server terminated: {}", e);
            }
        });
    }

    // Handle graceful shutdown
    let daemon_clone = daemon.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        info!("Received Ctrl+C, shutting down...");
        daemon_clone.stop();
    });

    if let Err(e) = daemon.start().await {
        error!("Daemon failed: {}", e);
        return Err(e);
    }

    info!("Daemon stopped");
    Ok(())
}
