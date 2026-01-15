//! Daemon bootstrap: wire supervisor(s), IPC server, and NullProxy
//!
//! This module provides a `bootstrap` function that loads service specs,
//! starts supervisors, spins up the IPC server with a supervisor control
//! plane, and installs graceful shutdown handling primitives.

use canopus_core::config::{load_services_from_toml_path, load_simple_services_from_toml_path};
use canopus_core::persistence::{
    default_snapshot_path, write_snapshot_atomic, RegistrySnapshot, ServiceSnapshot,
};
use canopus_core::proxy::ApiProxyAdapter;
use canopus_core::proxy_api::LocalReverseProxy;
use canopus_core::supervisor::{spawn_supervisor, SupervisorConfig, UnixProcessAdapter};
use schema::ServiceSpec;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::storage::SqliteStorage;
use crate::{DaemonError, Result};

/// Handle to manage the running components
#[allow(missing_debug_implementations)]
pub struct BootstrapHandle {
    #[allow(missing_docs)]
    pub services: Vec<ServiceSpec>,
    #[allow(missing_docs)]
    pub proxy: Arc<LocalReverseProxy>,
    server_task: Option<JoinHandle<ipc::Result<()>>>,
    handles: HashMap<String, canopus_core::supervisor::SupervisorHandle>,
}

/// Attempt to retrieve a login shell PATH suitable for user-level tools (npm, nvm shims, etc.)
///
/// Precedence:
/// - CANOPUS_LOGIN_PATH env var if set (explicit override)
/// - /bin/zsh -lc 'echo -n $PATH'
/// - /bin/bash -lc 'echo -n $PATH'
/// - Fallback: std::env::var("PATH")
fn resolve_login_path() -> Option<String> {
    if let Ok(v) = std::env::var("CANOPUS_LOGIN_PATH") {
        let t = v.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }

    // Try zsh interactive login shell (covers ~/.zprofile and ~/.zshrc where nvm is usually set)
    if let Ok(out) = StdCommand::new("/bin/zsh")
        .args(["-lic", "echo -n $PATH"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).to_string();
            if !s.trim().is_empty() {
                return Some(s);
            }
        }
    }

    // Fallback: zsh login-only
    if let Ok(out) = StdCommand::new("/bin/zsh")
        .args(["-lc", "echo -n $PATH"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).to_string();
            if !s.trim().is_empty() {
                return Some(s);
            }
        }
    }

    // Try bash login shell
    if let Ok(out) = StdCommand::new("/bin/bash")
        .args(["-lc", "echo -n $PATH"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).to_string();
            if !s.trim().is_empty() {
                return Some(s);
            }
        }
    }

    // Fallback to current process PATH
    std::env::var("PATH").ok()
}

impl BootstrapHandle {
    /// Initiate graceful shutdown: stop supervisors and abort IPC server task
    pub async fn shutdown(mut self) {
        for h in self.handles.values() {
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
    bootstrap_with_runtime(config_path, None, Some("127.0.0.1:80")).await
}

/// Bootstrap with an additional optional runtime config (simple per-service hostname/port)
pub async fn bootstrap_with_runtime(
    config_path: Option<PathBuf>,
    runtime_config_path: Option<PathBuf>,
    proxy_address: Option<&'static str>,
) -> Result<BootstrapHandle> {
    // Load services from config if provided, otherwise start empty (no supervisors)
    let services: Vec<ServiceSpec> = if let Some(path) = config_path {
        let cfg = load_services_from_toml_path(&path)
            .map_err(|e| DaemonError::ServerError(e.to_string()))?;
        cfg.services
    } else {
        vec![]
    };

    // Initialize local reverse proxy on provided address for friendly URLs like
    // http://service.localhost. Binding port 80 typically requires elevated
    // privileges (sudo/root). For testing, proxy_address can be set to None
    // to use an OS-assigned port.
    let proxy_addr = proxy_address.unwrap_or("127.0.0.1:0");
    let proxy = Arc::new(
        LocalReverseProxy::new(proxy_addr)
            .map_err(|e| DaemonError::ServerError(format!("proxy init failed: {}", e)))?,
    );

    // Initialize persistent storage (SQLite in $HOME/.canopus/canopus.db)
    let storage = SqliteStorage::open_default()
        .map_err(|e| DaemonError::ServerError(format!("storage init failed: {}", e)))?;

    // Shared event bus
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(1024);

    // Determine snapshot path and try loading existing snapshot
    let snapshot_path = default_snapshot_path();
    let mut registry_snap = match canopus_core::persistence::load_snapshot(&snapshot_path) {
        Ok(snap) => {
            warn!(
                "Loaded snapshot from {} ({} services). Not adopting PIDs.",
                snapshot_path.display(),
                snap.services.len()
            );
            // Do not adopt unknown PIDs; clear them
            let mut cleaned = snap.clone();
            for svc in cleaned.services.iter_mut() {
                svc.last_pid = None;
            }
            cleaned
        }
        Err(e) => {
            debug!("No valid snapshot loaded ({}). Starting clean.", e);
            RegistrySnapshot {
                version: canopus_core::persistence::SNAPSHOT_VERSION,
                timestamp: canopus_core::persistence::current_timestamp(),
                services: vec![],
            }
        }
    };

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
            proxy_adapter: Arc::new(ApiProxyAdapter::new(proxy.clone())),
        };
        let handle = spawn_supervisor(cfg);
        // Recovery workflow: auto-start only services with Always policy
        if spec.restart_policy == schema::RestartPolicy::Always {
            let _ = handle.start();
        } else {
            debug!(
                "Service '{}' not auto-started (policy: {:?})",
                spec.id, spec.restart_policy
            );
        }
        handles.insert(spec.id.clone(), handle);

        // Seed persistent storage row for this service (Idle, no PID)
        if let Err(e) = storage
            .upsert_service(
                &spec.id,
                &spec.name,
                &format!("{:?}", schema::ServiceState::Idle),
                None,
                None,
                None,
            )
            .await
        {
            warn!("Failed to seed storage for service {}: {}", spec.id, e);
        }

        // Seed registry snapshot entry for this service if missing
        if !registry_snap.services.iter().any(|s| s.id == spec.id) {
            registry_snap.services.push(ServiceSnapshot {
                id: spec.id.clone(),
                spec: spec.clone(),
                last_state: schema::ServiceState::Idle,
                last_pid: None,
            });
        }
    }

    // Write initial snapshot reflecting loaded specs and baseline states
    if let Err(e) = write_snapshot_atomic(&snapshot_path, &registry_snap) {
        warn!("Failed to write initial snapshot: {}", e);
    }

    // If a runtime config is provided, preload hostname/port into SQLite for known services
    if let Some(rt_path) = runtime_config_path {
        match load_simple_services_from_toml_path(&rt_path) {
            Ok(simple) => {
                for (id, scfg) in simple.services.iter() {
                    if handles.contains_key(id) {
                        if let Some(p) = scfg.port {
                            if let Err(e) = storage.update_port(id, Some(p)).await {
                                warn!("Failed to persist runtime port for {}: {}", id, e);
                            }
                        }
                        if let Some(hn) = scfg.hostname.as_deref() {
                            if let Err(e) = storage.update_hostname(id, Some(hn)).await {
                                warn!("Failed to persist runtime hostname for {}: {}", id, e);
                            }
                        }
                    } else {
                        warn!(
                            "Runtime config references unknown service '{}'; ignoring",
                            id
                        );
                    }
                }
            }
            Err(e) => warn!("Failed to load runtime config {:?}: {}", rt_path, e),
        }
    }

    // Start IPC server (UDS on Unix)
    let server_task = {
        #[cfg(unix)]
        {
            use ipc::server::supervisor_adapter::SupervisorControlPlane;
            use ipc::server::{IpcServer, IpcServerConfig};
            let socket_path = std::env::var("CANOPUS_IPC_SOCKET")
                .unwrap_or_else(|_| "/tmp/canopus.sock".to_string());
            let token = std::env::var("CANOPUS_IPC_TOKEN").ok();
            let cfg = IpcServerConfig {
                version: env!("CARGO_PKG_VERSION").to_string(),
                auth_token: token,
                unix_socket_path: Some(PathBuf::from(socket_path)),
                windows_pipe_name: None,
            };
            use std::sync::Arc as StdArc;
            // Resolve a login PATH so spawned services can resolve user-level binaries like npm
            let login_path = resolve_login_path();
            let router = SupervisorControlPlane::new(handles.clone(), event_tx.clone())
                .with_meta_store(StdArc::new(storage.clone()))
                .with_login_path(login_path);
            let server = IpcServer::with_router(cfg, Arc::new(router));
            Some(tokio::spawn(async move { server.serve().await }))
        }
        #[cfg(not(unix))]
        {
            None
        }
    };

    // Spawn snapshot writer task: subscribe to events and update snapshot on changes
    {
        let mut rx = event_tx.subscribe();
        let snapshot_path = snapshot_path.clone();
        let mut registry = registry_snap;
        let storage = storage.clone();
        tokio::spawn(async move {
            while let Ok(evt) = rx.recv().await {
                // Update registry entries based on event
                match &evt {
                    schema::ServiceEvent::StateChanged {
                        service_id,
                        to_state,
                        ..
                    } => {
                        if let Some(e) = registry.services.iter_mut().find(|e| &e.id == service_id)
                        {
                            e.last_state = *to_state;
                        }
                        // Persist to SQLite
                        if let Err(e) = storage
                            .update_state(service_id, &format!("{:?}", to_state))
                            .await
                        {
                            warn!("Failed to persist state for {}: {}", service_id, e);
                        }
                    }
                    schema::ServiceEvent::ProcessStarted {
                        service_id, pid, ..
                    } => {
                        if let Some(e) = registry.services.iter_mut().find(|e| &e.id == service_id)
                        {
                            e.last_pid = Some(*pid);
                        }
                        if let Err(e) = storage.update_pid(service_id, Some(*pid)).await {
                            warn!("Failed to persist pid for {}: {}", service_id, e);
                        }
                    }
                    schema::ServiceEvent::ProcessExited { service_id, .. } => {
                        if let Some(e) = registry.services.iter_mut().find(|e| &e.id == service_id)
                        {
                            e.last_pid = None;
                        }
                        if let Err(e) = storage.update_pid(service_id, None).await {
                            warn!("Failed to clear pid for {}: {}", service_id, e);
                        }
                        // Clear port on exit since it's no longer occupied
                        if let Err(e) = storage.update_port(service_id, None).await {
                            warn!("Failed to clear port for {}: {}", service_id, e);
                        }
                    }
                    _ => {}
                }

                // Refresh timestamp and persist
                registry.timestamp = canopus_core::persistence::current_timestamp();
                if let Err(e) = write_snapshot_atomic(&snapshot_path, &registry) {
                    warn!("Snapshot write failed: {}", e);
                }
            }
        });
    }

    if services.is_empty() {
        warn!("No services configured; IPC will still run if enabled");
    }

    Ok(BootstrapHandle {
        services,
        proxy,
        server_task,
        handles,
    })
}
