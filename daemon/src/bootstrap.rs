//! Daemon bootstrap: wire supervisor(s), IPC server, and NullProxy
//!
//! This module provides a `bootstrap` function that loads service specs,
//! starts supervisors, spins up the IPC server with a supervisor control
//! plane, and installs graceful shutdown handling primitives.

use canopus_core::config::load_services_from_toml_path;
use canopus_core::proxy_api::NullProxy;
use canopus_core::supervisor::{spawn_supervisor, SupervisorConfig, UnixProcessAdapter};
use schema::ServiceSpec;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::{DaemonError, Result};

/// Handle to manage the running components
pub struct BootstrapHandle {
    pub services: Vec<ServiceSpec>,
    pub proxy: Arc<NullProxy>,
    server_task: Option<JoinHandle<ipc::Result<()>>>,
    handles: HashMap<String, canopus_core::supervisor::SupervisorHandle>,
}

impl BootstrapHandle {
    /// Initiate graceful shutdown: stop supervisors and abort IPC server task
    pub async fn shutdown(mut self) {
        for (_id, h) in &self.handles {
            let _ = h.shutdown();
        }
        if let Some(task) = self.server_task.take() {
            // IPC server runs accept loop; aborting is acceptable for now
            task.abort();
        }
        info!("Bootstrap shutdown complete");
    }
}

/// Bootstrap the daemon components
pub async fn bootstrap(config_path: Option<PathBuf>) -> Result<BootstrapHandle> {
    // Load services from config if provided, otherwise start empty (no supervisors)
    let services: Vec<ServiceSpec> = if let Some(path) = config_path {
        let cfg = load_services_from_toml_path(&path)
            .map_err(|e| DaemonError::ServerError(e.to_string()))?;
        cfg.services
    } else {
        vec![]
    };

    let proxy = Arc::new(NullProxy::new());

    // Shared event bus
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(1024);

    // Spawn supervisors
    let mut handles = HashMap::new();
    #[cfg(unix)]
    let adapter = Arc::new(UnixProcessAdapter::new());
    #[cfg(not(unix))]
    let adapter = Arc::new(UnixProcessAdapter::new()); // placeholder; non-unix adapter can be added later

    for spec in services.iter().cloned() {
        let cfg = SupervisorConfig {
            spec: spec.clone(),
            process_adapter: adapter.clone(),
            event_tx: event_tx.clone(),
        };
        let handle = spawn_supervisor(cfg);
        let _ = handle.start();
        handles.insert(spec.id.clone(), handle);
    }

    // Start IPC server (UDS on Unix)
    let mut server_task = None;
    #[cfg(unix)]
    {
        use ipc::server::{IpcServer, IpcServerConfig};
        use ipc::server::supervisor_adapter::SupervisorControlPlane;
        let socket_path = std::env::var("CANOPUS_IPC_SOCKET").unwrap_or_else(|_| "/tmp/canopus.sock".to_string());
        let token = std::env::var("CANOPUS_IPC_TOKEN").ok();
        let cfg = IpcServerConfig {
            version: env!("CARGO_PKG_VERSION").to_string(),
            auth_token: token,
            unix_socket_path: Some(std::path::PathBuf::from(socket_path)),
            windows_pipe_name: None,
        };
        let router = SupervisorControlPlane::new(handles.clone(), event_tx.clone());
        let server = IpcServer::with_router(cfg, Arc::new(router));
        server_task = Some(tokio::spawn(async move { server.serve().await }));
    }

    if services.is_empty() {
        warn!("No services configured; IPC will still run if enabled");
    }

    Ok(BootstrapHandle { services, proxy, server_task, handles })
}
