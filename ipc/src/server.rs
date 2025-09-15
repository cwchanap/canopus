//! Local IPC JSON-RPC server over UDS/Named Pipe
//!
//! Provides a simple JSON-RPC 2.0 server that listens on a Unix Domain Socket
//! (Unix) or Windows Named Pipe (Windows). Connections must perform a handshake
//! before invoking methods. Handshake includes daemon version and optional
//! bearer token verification.
//!
//! Methods are namespaced as `canopus.*` and currently implemented as stubs:
//! - `canopus.list`
//! - `canopus.start`
//! - `canopus.stop`
//! - `canopus.restart`
//! - `canopus.status`
//! - `canopus.bindHost`
//! - `canopus.assignPort`
//! - `canopus.tailLogs` (streaming skeleton)
//! - `canopus.healthCheck`
//! - `canopus.version`

use crate::{IpcError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// JSON-RPC 2.0 request
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    id: Option<Value>,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    #[serde(default)]
    id: Option<Value>,
}

/// JSON-RPC error object
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcResponse {
    fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }
    fn err(id: Option<Value>, code: i32, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data,
            }),
            id,
        }
    }
}

/// Configuration for the local IPC server
#[derive(Debug, Clone)]
pub struct IpcServerConfig {
    /// Daemon semantic version (e.g., 0.1.0)
    pub version: String,
    /// Optional bearer token required for handshake
    pub auth_token: Option<String>,
    /// Unix socket path (unix only)
    pub unix_socket_path: Option<std::path::PathBuf>,
    /// Windows named pipe name (windows only), e.g. \\?\pipe\canopus
    pub windows_pipe_name: Option<String>,
}

impl Default for IpcServerConfig {
    fn default() -> Self {
        Self {
            version: "0.1.0".to_string(),
            auth_token: None,
            unix_socket_path: None,
            windows_pipe_name: None,
        }
    }
}

/// IPC server entry
#[allow(missing_debug_implementations)]
pub struct IpcServer {
    config: IpcServerConfig,
    router: Arc<dyn ControlPlane>,
}

impl IpcServer {
    /// Create a new IPC server
    pub fn new(config: IpcServerConfig) -> Self {
        Self {
            config,
            router: Arc::new(NoopControlPlane),
        }
    }

    /// Create with a provided control plane/router implementation
    pub fn with_router(config: IpcServerConfig, router: Arc<dyn ControlPlane>) -> Self {
        Self { config, router }
    }

    /// Start serving connections based on platform
    pub async fn serve(&self) -> Result<()> {
        #[cfg(unix)]
        {
            self.serve_unix().await
        }
        #[cfg(windows)]
        {
            self.serve_windows().await
        }
        #[cfg(all(not(unix), not(windows)))]
        {
            Err(IpcError::ProtocolError(
                "Unsupported platform for local IPC".to_string(),
            ))
        }
    }

    #[cfg(unix)]
    async fn serve_unix(&self) -> Result<()> {
        use tokio::net::UnixListener;
        let path = self.config.unix_socket_path.clone().ok_or_else(|| {
            IpcError::ProtocolError("unix_socket_path is required on Unix".to_string())
        })?;

        // Remove pre-existing socket file if present
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(_) => debug!("Removed existing socket at {:?}", path),
                Err(e) => {
                    return Err(IpcError::ProtocolError(format!(
                        "Failed to remove existing socket {:?}: {}",
                        path, e
                    )));
                }
            }
        }

        let listener = UnixListener::bind(&path).map_err(|e| {
            IpcError::ConnectionFailed(format!("Failed to bind UDS {:?}: {}", path, e))
        })?;
        info!("IPC server (UDS) listening at {:?}", path);

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let cfg = self.config.clone();
                    let router = self.router.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection_unix(stream, cfg, router).await {
                            warn!("UDS connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept UDS connection: {}", e);
                }
            }
        }
    }

    #[cfg(windows)]
    async fn serve_windows(&self) -> Result<()> {
        // Named pipe server skeleton: implement in a subsequent task
        Err(IpcError::ProtocolError(
            "Windows Named Pipe server not yet implemented".to_string(),
        ))
    }
}

#[cfg(unix)]
async fn handle_connection_unix(
    stream: tokio::net::UnixStream,
    config: IpcServerConfig,
    router: Arc<dyn ControlPlane>,
) -> Result<()> {
    // Handshake: expect a JSON-RPC call canopus.handshake with optional token
    let (mut reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));

    let req = read_request(&mut reader).await?;
    if req.jsonrpc != "2.0" || req.method != "canopus.handshake" {
        let resp = JsonRpcResponse::err(
            req.id,
            -32600,
            "Handshake required (canopus.handshake)",
            None,
        );
        write_response_locked(writer.clone(), &resp).await?;
        return Err(IpcError::ProtocolError("Missing handshake".to_string()));
    }

    // Validate token if configured
    let provided = req
        .params
        .as_ref()
        .and_then(|v| v.get("token"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(expected) = config.auth_token.clone() {
        if provided.as_deref() != Some(expected.as_str()) {
            let resp = JsonRpcResponse::err(req.id, -32001, "Authentication failed", None);
            write_response_locked(writer.clone(), &resp).await?;
            return Err(IpcError::ProtocolError("Auth failed".to_string()));
        }
    }

    // Respond with version in handshake result
    let resp = JsonRpcResponse::ok(req.id, serde_json::json!({"version": config.version}));
    write_response_locked(writer.clone(), &resp).await?;

    // Now enter request loop
    loop {
        let req = match read_request(&mut reader).await {
            Ok(r) => r,
            Err(IpcError::EmptyResponse) => break,
            Err(e) => return Err(e),
        };

        if let Some(reply) = route_method(&config, router.clone(), writer.clone(), req).await? {
            write_response_locked(writer.clone(), &reply).await?;
        }
    }

    Ok(())
}

async fn route_method(
    config: &IpcServerConfig,
    router: Arc<dyn ControlPlane>,
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    req: JsonRpcRequest,
) -> Result<Option<JsonRpcResponse>> {
    let id = req.id.clone();
    let params = req.params.unwrap_or(Value::Null);
    let method = req.method.as_str();

    #[allow(unused_macros)]
    macro_rules! bad_params {
        ($msg:expr) => {
            return Ok(Some(JsonRpcResponse::err(id, -32602, $msg, None)));
        };
    }

    Ok(Some(match method {
        "canopus.version" => {
            JsonRpcResponse::ok(id, serde_json::json!({"version": config.version }))
        }
        "canopus.list" => {
            let services = router.list().await?;
            JsonRpcResponse::ok(id, serde_json::json!({"services": services}))
        }
        "canopus.status" => {
            let sid = params
                .get("serviceId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            let detail = router.status(sid).await?;
            JsonRpcResponse::ok(id, serde_json::to_value(detail).unwrap())
        }
        "canopus.start" => {
            let sid = params
                .get("serviceId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            router.start(sid).await?;
            JsonRpcResponse::ok(id, serde_json::json!({"ok": true}))
        }
        "canopus.stop" => {
            let sid = params
                .get("serviceId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            router.stop(sid).await?;
            JsonRpcResponse::ok(id, serde_json::json!({"ok": true}))
        }
        "canopus.restart" => {
            let sid = params
                .get("serviceId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            router.restart(sid).await?;
            JsonRpcResponse::ok(id, serde_json::json!({"ok": true}))
        }
        "canopus.bindHost" => {
            let sid = params
                .get("serviceId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            let host = params
                .get("host")
                .and_then(|v| v.as_str())
                .ok_or_else(|| IpcError::ProtocolError("missing host".into()))?;
            router.bind_host(sid, host).await?;
            JsonRpcResponse::ok(id, serde_json::json!({"ok": true}))
        }
        "canopus.assignPort" => {
            let sid = params
                .get("serviceId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            let preferred = params
                .get("preferred")
                .and_then(|v| v.as_u64())
                .map(|n| n as u16);
            let port = router.assign_port(sid, preferred).await?;
            JsonRpcResponse::ok(id, serde_json::json!({"port": port}))
        }
        "canopus.healthCheck" => {
            let sid = params
                .get("serviceId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            let healthy = router.health_check(sid).await?;
            JsonRpcResponse::ok(id, serde_json::json!({"healthy": healthy}))
        }
        "canopus.tailLogs" => {
            let sid = params
                .get("serviceId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?
                .to_string();
            let from_seq = params.get("fromSeq").and_then(|v| v.as_u64());
            let mut rx = router.tail_logs(&sid, from_seq).await?;
            let writer_clone = writer.clone();
            tokio::spawn(async move {
                while let Some(evt) = rx.recv().await {
                    let _ = write_notification_locked(
                        writer_clone.clone(),
                        "canopus.tailLogs.update",
                        serde_json::to_value(&evt).unwrap(),
                    )
                    .await;
                }
            });
            JsonRpcResponse::ok(id, serde_json::json!({"subscribed": true}))
        }
        _ => JsonRpcResponse::err(id, -32601, "Method not found", None),
    }))
}

async fn read_request<S: AsyncReadExt + Unpin>(stream: &mut S) -> Result<JsonRpcRequest> {
    // Simple framing: read up to 64KB or until EOF; for real-world use adopt length-prefix or newline-delimited framing
    let mut buf = vec![0u8; 65536];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| IpcError::ReceiveFailed(e.to_string()))?;
    if n == 0 {
        return Err(IpcError::EmptyResponse);
    }
    serde_json::from_slice::<JsonRpcRequest>(&buf[..n])
        .map_err(|e| IpcError::DeserializationFailed(e.to_string()))
}

async fn write_response_locked(
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    resp: &JsonRpcResponse,
) -> Result<()> {
    let data =
        serde_json::to_vec(resp).map_err(|e| IpcError::SerializationFailed(e.to_string()))?;
    let mut guard = writer.lock().await;
    guard
        .write_all(&data)
        .await
        .map_err(|e| IpcError::SendFailed(e.to_string()))
}

async fn write_notification_locked(
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    method: &str,
    params: Value,
) -> Result<()> {
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    let data =
        serde_json::to_vec(&notif).map_err(|e| IpcError::SerializationFailed(e.to_string()))?;
    let mut guard = writer.lock().await;
    guard
        .write_all(&data)
        .await
        .map_err(|e| IpcError::SendFailed(e.to_string()))
}

/// Abstract control plane that the IPC server delegates to
#[async_trait::async_trait]
#[allow(missing_docs)]
pub trait ControlPlane: Send + Sync {
    async fn list(&self) -> Result<Vec<ServiceSummary>>;
    async fn start(&self, service_id: &str) -> Result<()>;
    async fn stop(&self, service_id: &str) -> Result<()>;
    async fn restart(&self, service_id: &str) -> Result<()>;
    async fn status(&self, service_id: &str) -> Result<ServiceDetail>;
    async fn bind_host(&self, service_id: &str, host: &str) -> Result<()>;
    async fn assign_port(&self, service_id: &str, preferred: Option<u16>) -> Result<u16>;
    async fn health_check(&self, service_id: &str) -> Result<bool>;
    async fn tail_logs(
        &self,
        service_id: &str,
        from_seq: Option<u64>,
    ) -> Result<mpsc::Receiver<schema::ServiceEvent>>;
}

/// Minimal summary for listing services
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSummary {
    pub id: String,
    pub name: String,
    pub state: schema::ServiceState,
}

/// Detailed service status for a specific service
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDetail {
    pub id: String,
    pub name: String,
    pub state: schema::ServiceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

struct NoopControlPlane;

#[async_trait::async_trait]
impl ControlPlane for NoopControlPlane {
    async fn list(&self) -> Result<Vec<ServiceSummary>> {
        Ok(vec![])
    }
    async fn start(&self, _service_id: &str) -> Result<()> {
        Err(IpcError::ProtocolError("start not implemented".into()))
    }
    async fn stop(&self, _service_id: &str) -> Result<()> {
        Err(IpcError::ProtocolError("stop not implemented".into()))
    }
    async fn restart(&self, _service_id: &str) -> Result<()> {
        Err(IpcError::ProtocolError("restart not implemented".into()))
    }
    async fn status(&self, _service_id: &str) -> Result<ServiceDetail> {
        Err(IpcError::ProtocolError("status not implemented".into()))
    }
    async fn bind_host(&self, _service_id: &str, _host: &str) -> Result<()> {
        Err(IpcError::ProtocolError("bindHost not implemented".into()))
    }
    async fn assign_port(&self, _service_id: &str, _preferred: Option<u16>) -> Result<u16> {
        Err(IpcError::ProtocolError("assignPort not implemented".into()))
    }
    async fn health_check(&self, _service_id: &str) -> Result<bool> {
        Err(IpcError::ProtocolError(
            "healthCheck not implemented".into(),
        ))
    }
    async fn tail_logs(
        &self,
        _service_id: &str,
        _from_seq: Option<u64>,
    ) -> Result<mpsc::Receiver<schema::ServiceEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }
}

#[cfg(feature = "supervisor")]
#[allow(missing_docs)]
pub mod supervisor_adapter {
    use super::{ControlPlane, Result, ServiceDetail, ServiceSummary};
    use async_trait::async_trait;
    use canopus_core::supervisor::SupervisorHandle;
    use schema::ServiceEvent;
    use std::collections::HashMap;
    use tokio::sync::{broadcast, mpsc};

    /// Control plane adapter backed by a set of SupervisorHandles and a shared event bus
    #[derive(Debug)]
    pub struct SupervisorControlPlane {
        handles: HashMap<String, SupervisorHandle>,
        event_tx: broadcast::Sender<ServiceEvent>,
    }

    impl SupervisorControlPlane {
        pub fn new(
            handles: HashMap<String, SupervisorHandle>,
            event_tx: broadcast::Sender<ServiceEvent>,
        ) -> Self {
            Self { handles, event_tx }
        }
    }

    #[async_trait]
    impl ControlPlane for SupervisorControlPlane {
        async fn list(&self) -> Result<Vec<ServiceSummary>> {
            let mut out = Vec::with_capacity(self.handles.len());
            for (id, h) in &self.handles {
                out.push(ServiceSummary {
                    id: id.clone(),
                    name: h.spec.name.clone(),
                    state: h.current_state(),
                });
            }
            Ok(out)
        }

        async fn start(&self, service_id: &str) -> Result<()> {
            let handle = self
                .handles
                .get(service_id)
                .ok_or_else(|| super::IpcError::ProtocolError("unknown service".into()))?;

            // Subscribe to state changes before starting to avoid missing Ready transition
            let mut state_rx = handle.subscribe_to_state();

            handle
                .start()
                .map_err(|e| super::IpcError::ProtocolError(e.to_string()))?;

            // Default wait-ready behavior: wait until service reaches Ready state
            let current = *state_rx.borrow();
            if current == schema::ServiceState::Ready {
                return Ok(());
            }

            let timeout_secs = (handle.spec.startup_timeout_secs as u64).saturating_add(5);
            let wait = async {
                loop {
                    if state_rx.changed().await.is_err() {
                        return Err(());
                    }
                    if *state_rx.borrow() == schema::ServiceState::Ready {
                        return Ok(());
                    }
                }
            };

            match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), wait).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(())) => Err(super::IpcError::ProtocolError(
                    "service state channel closed".into(),
                )),
                Err(_) => Err(super::IpcError::ProtocolError(
                    "timed out waiting for service readiness".into(),
                )),
            }
        }

        async fn status(&self, service_id: &str) -> Result<ServiceDetail> {
            let handle = self
                .handles
                .get(service_id)
                .ok_or_else(|| super::IpcError::ProtocolError("unknown service".into()))?;
            let pid = handle
                .get_pid()
                .await
                .map_err(|e| super::IpcError::ProtocolError(e.to_string()))?;
            Ok(ServiceDetail {
                id: service_id.to_string(),
                name: handle.spec.name.clone(),
                state: handle.current_state(),
                pid,
            })
        }

        async fn stop(&self, service_id: &str) -> Result<()> {
            self.handles
                .get(service_id)
                .ok_or_else(|| super::IpcError::ProtocolError("unknown service".into()))?
                .stop()
                .map_err(|e| super::IpcError::ProtocolError(e.to_string()))
        }

        async fn restart(&self, service_id: &str) -> Result<()> {
            self.handles
                .get(service_id)
                .ok_or_else(|| super::IpcError::ProtocolError("unknown service".into()))?
                .restart()
                .map_err(|e| super::IpcError::ProtocolError(e.to_string()))
        }

        async fn bind_host(&self, _service_id: &str, _host: &str) -> Result<()> {
            // No-op for now; binding handled externally
            Ok(())
        }

        async fn assign_port(&self, _service_id: &str, _preferred: Option<u16>) -> Result<u16> {
            // Allocate a free port best-effort
            let alloc = canopus_core::PortAllocator::new();
            let port = match alloc.reserve(_preferred) {
                Ok(g) => g.port(),
                Err(_) => 0,
            };
            Ok(port)
        }

        async fn health_check(&self, service_id: &str) -> Result<bool> {
            let handle = self
                .handles
                .get(service_id)
                .ok_or_else(|| super::IpcError::ProtocolError("unknown service".into()))?;
            handle
                .is_healthy()
                .await
                .map_err(|e| super::IpcError::ProtocolError(e.to_string()))
        }

        async fn tail_logs(
            &self,
            service_id: &str,
            _from_seq: Option<u64>,
        ) -> Result<mpsc::Receiver<ServiceEvent>> {
            let mut rx = self.event_tx.subscribe();
            let (tx, out_rx) = mpsc::channel(100);
            let sid = service_id.to_string();
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(evt) => {
                            if evt.service_id() == sid {
                                // Forward only log outputs and optionally others
                                if matches!(evt, ServiceEvent::LogOutput { .. }) {
                                    let _ = tx.send(evt).await;
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            // Skip lagged; continue
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            Ok(out_rx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unix_server_handshake_rejects_without_token_when_required() {
        #[cfg(unix)]
        {
            use tokio::net::UnixStream;
            let dir = tempfile::tempdir().unwrap();
            let sock = dir.path().join("canopus.sock");
            let cfg = IpcServerConfig {
                version: "0.1.0".to_string(),
                auth_token: Some("secret".to_string()),
                unix_socket_path: Some(sock.clone()),
                windows_pipe_name: None,
            };

            let server = IpcServer::new(cfg.clone());
            tokio::spawn(async move {
                let _ = server.serve_unix().await;
            });
            // Give server a moment
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let mut stream = UnixStream::connect(&sock).await.unwrap();
            // Send handshake without token
            let req = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "canopus.handshake",
                "id": 1
            });
            let data = serde_json::to_vec(&req).unwrap();
            stream.write_all(&data).await.unwrap();

            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            assert!(n > 0);
            let v: Value = serde_json::from_slice(&buf[..n]).unwrap();
            assert!(v.get("error").is_some());
        }
    }
}
