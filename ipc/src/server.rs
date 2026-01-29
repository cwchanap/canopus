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
use schema::ServiceEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

#[cfg(unix)]
use nix::unistd::{chown, Group};

/// Metadata store for per-service runtime info (e.g., port, hostname)
#[async_trait::async_trait]
pub trait ServiceMetaStore: Send + Sync {
    /// Set or remove the port for a service
    async fn set_port(&self, service_id: &str, port: Option<u16>) -> Result<()>;
    /// Set or remove the hostname for a service
    async fn set_hostname(&self, service_id: &str, hostname: Option<&str>) -> Result<()>;
    /// Retrieve the port for a service
    async fn get_port(&self, service_id: &str) -> Result<Option<u16>>;
    /// Retrieve the hostname for a service
    async fn get_hostname(&self, service_id: &str) -> Result<Option<String>>;
    /// Delete the metadata row for a service
    async fn delete(&self, service_id: &str) -> Result<()>;
}

#[cfg(unix)]
fn configure_socket_permissions(path: &std::path::Path) -> Result<()> {
    use std::env;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let group_name = env::var("CANOPUS_SOCKET_GROUP")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "canopus".to_string());

    let group = match Group::from_name(&group_name) {
        Ok(Some(group)) => group,
        Ok(None) => {
            warn!(
                "Socket group '{}' not found; leaving permissions as default",
                group_name
            );
            return Ok(());
        }
        Err(e) => {
            warn!(
                "Failed to resolve socket group '{}': {}; leaving permissions as default",
                group_name, e
            );
            return Ok(());
        }
    };

    chown(path, None, Some(group.gid)).map_err(|e| {
        IpcError::ProtocolError(format!("Failed to chown socket {}: {e}", path.display()))
    })?;

    let metadata = fs::metadata(path).map_err(|e| {
        IpcError::ProtocolError(format!("Failed to stat socket {}: {e}", path.display()))
    })?;
    let mut perms = metadata.permissions();
    perms.set_mode(0o660);
    fs::set_permissions(path, perms).map_err(|e| {
        IpcError::ProtocolError(format!(
            "Failed to set permissions on {}: {e}",
            path.display()
        ))
    })?;

    info!(
        "Configured IPC socket {:?} with group '{}' and mode 660",
        path, group_name
    );

    Ok(())
}

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
    #[must_use]
    pub fn new(config: IpcServerConfig) -> Self {
        Self {
            config,
            router: Arc::new(NoopControlPlane),
        }
    }

    /// Create with a provided control plane/router implementation
    #[must_use]
    pub fn with_router(config: IpcServerConfig, router: Arc<dyn ControlPlane>) -> Self {
        Self { config, router }
    }

    /// Start serving connections based on platform
    ///
    /// # Errors
    ///
    /// Returns an error if the platform is unsupported or the server cannot start.
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
                Ok(()) => debug!("Removed existing socket at {:?}", path),
                Err(e) => {
                    return Err(IpcError::ProtocolError(format!(
                        "Failed to remove existing socket {}: {e}",
                        path.display()
                    )));
                }
            }
        }

        let listener = UnixListener::bind(&path).map_err(|e| {
            IpcError::ConnectionFailed(format!("Failed to bind UDS {}: {e}", path.display()))
        })?;
        info!("IPC server (UDS) listening at {:?}", path);

        if let Err(e) = configure_socket_permissions(&path) {
            warn!(
                "Unable to configure socket permissions for {:?}: {}",
                path, e
            );
        }

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
    let (reader, writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
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
        .and_then(Value::as_str)
        .map(ToString::to_string);

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

#[allow(clippy::too_many_lines)]
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
        ($field:expr, $msg:expr) => {{
            return Ok(Some(JsonRpcResponse::err(
                id,
                -32602,
                format!("invalid params: {} {}", $field, $msg),
                None,
            )));
        }};
        ($msg:expr) => {{
            return Ok(Some(JsonRpcResponse::err(id, -32602, $msg, None)));
        }};
    }

    /// Helper macro to parse an optional port parameter from JSON
    #[allow(unused_macros)]
    macro_rules! parse_optional_port {
        ($field_name:expr, $value:expr) => {{
            match $value {
                Some(v) if v.is_null() => None,
                Some(v) => {
                    let Some(n) = v.as_u64() else {
                        bad_params!($field_name, "invalid port number");
                    };
                    let Ok(port) = u16::try_from(n) else {
                        bad_params!($field_name, "invalid port number");
                    };
                    Some(port)
                }
                None => None,
            }
        }};
    }

    Ok(Some(match method {
        "canopus.version" => {
            JsonRpcResponse::ok(id, serde_json::json!({"version": config.version }))
        }
        "canopus.list" => match router.list().await {
            Ok(services) => JsonRpcResponse::ok(id, serde_json::json!({"services": services})),
            Err(e) => JsonRpcResponse::err(id, -32000, format!("list failed: {e}"), None),
        },
        "canopus.status" => {
            let sid = params
                .get("serviceId")
                .and_then(Value::as_str)
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            match router.status(sid).await {
                Ok(detail) => JsonRpcResponse::ok(id, serde_json::to_value(detail).unwrap()),
                Err(e) => JsonRpcResponse::err(id, -32000, format!("status failed: {e}"), None),
            }
        }
        "canopus.start" => {
            let sid = params
                .get("serviceId")
                .and_then(Value::as_str)
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            let port = parse_optional_port!("port", params.get("port"));
            let hostname = params
                .get("hostname")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            match router.start(sid, port, hostname).await {
                Ok(()) => JsonRpcResponse::ok(id, serde_json::json!({"ok": true})),
                Err(e) => JsonRpcResponse::err(id, -32000, format!("start failed: {e}"), None),
            }
        }
        "canopus.stop" => {
            let sid = params
                .get("serviceId")
                .and_then(Value::as_str)
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            match router.stop(sid).await {
                Ok(()) => JsonRpcResponse::ok(id, serde_json::json!({"ok": true})),
                Err(e) => JsonRpcResponse::err(id, -32000, format!("stop failed: {e}"), None),
            }
        }
        "canopus.restart" => {
            let sid = params
                .get("serviceId")
                .and_then(Value::as_str)
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            match router.restart(sid).await {
                Ok(()) => JsonRpcResponse::ok(id, serde_json::json!({"ok": true})),
                Err(e) => JsonRpcResponse::err(id, -32000, format!("restart failed: {e}"), None),
            }
        }
        "canopus.bindHost" => {
            let sid = params
                .get("serviceId")
                .and_then(Value::as_str)
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            let host = params
                .get("host")
                .and_then(Value::as_str)
                .ok_or_else(|| IpcError::ProtocolError("missing host".into()))?;
            match router.bind_host(sid, host).await {
                Ok(()) => JsonRpcResponse::ok(id, serde_json::json!({"ok": true})),
                Err(e) => JsonRpcResponse::err(id, -32000, format!("bindHost failed: {e}"), None),
            }
        }
        "canopus.assignPort" => {
            let sid = params
                .get("serviceId")
                .and_then(Value::as_str)
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            let preferred = parse_optional_port!("preferred", params.get("preferred"));
            match router.assign_port(sid, preferred).await {
                Ok(port) => JsonRpcResponse::ok(id, serde_json::json!({"port": port})),
                Err(e) => JsonRpcResponse::err(id, -32000, format!("assignPort failed: {e}"), None),
            }
        }
        "canopus.healthCheck" => {
            let sid = params
                .get("serviceId")
                .and_then(Value::as_str)
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            match router.health_check(sid).await {
                Ok(healthy) => JsonRpcResponse::ok(id, serde_json::json!({"healthy": healthy})),
                Err(e) => {
                    JsonRpcResponse::err(id, -32000, format!("healthCheck failed: {e}"), None)
                }
            }
        }
        "canopus.tailLogs" => {
            let sid = params
                .get("serviceId")
                .and_then(Value::as_str)
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?
                .to_string();
            let from_seq = params.get("fromSeq").and_then(Value::as_u64);
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
        "canopus.deleteMeta" => {
            let sid = params
                .get("serviceId")
                .and_then(Value::as_str)
                .ok_or_else(|| IpcError::ProtocolError("missing serviceId".into()))?;
            router.delete_meta(sid).await?;
            JsonRpcResponse::ok(id, serde_json::json!({"ok": true}))
        }
        _ => JsonRpcResponse::err(id, -32601, "Method not found", None),
    }))
}

/// Maximum allowed frame size for IPC requests (64KB)
/// This prevents unbounded memory growth from malicious clients
const MAX_FRAME_SIZE: usize = 64 * 1024;

async fn read_request<S: tokio::io::AsyncBufRead + Unpin>(
    stream: &mut S,
) -> Result<JsonRpcRequest> {
    // Simple framing: read one JSON value per line with bounded buffering.
    let mut buf = Vec::with_capacity(1024);
    loop {
        let chunk = stream
            .fill_buf()
            .await
            .map_err(|e| IpcError::ReceiveFailed(e.to_string()))?;
        if chunk.is_empty() {
            return Err(IpcError::EmptyResponse);
        }

        let newline_pos = chunk.iter().position(|b| *b == b'\n');
        let to_copy = newline_pos.map_or(chunk.len(), |idx| idx + 1);
        let next_len = buf.len() + to_copy;
        if next_len > MAX_FRAME_SIZE {
            return Err(IpcError::ProtocolError(format!(
                "Frame size {next_len} exceeds maximum allowed size of {MAX_FRAME_SIZE} bytes"
            )));
        }

        buf.extend_from_slice(&chunk[..to_copy]);
        stream.consume(to_copy);
        if newline_pos.is_some() {
            break;
        }
    }

    if matches!(buf.last(), Some(b'\n')) {
        buf.pop();
        if matches!(buf.last(), Some(b'\r')) {
            buf.pop();
        }
    }
    serde_json::from_slice::<JsonRpcRequest>(&buf)
        .map_err(|e| IpcError::DeserializationFailed(e.to_string()))
}

async fn write_response_locked(
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    resp: &JsonRpcResponse,
) -> Result<()> {
    let mut data =
        serde_json::to_vec(resp).map_err(|e| IpcError::SerializationFailed(e.to_string()))?;
    data.push(b'\n');
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
    let mut data =
        serde_json::to_vec(&notif).map_err(|e| IpcError::SerializationFailed(e.to_string()))?;
    data.push(b'\n');
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
    async fn start(
        &self,
        service_id: &str,
        port: Option<u16>,
        hostname: Option<String>,
    ) -> Result<()>;
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
    ) -> Result<mpsc::Receiver<ServiceEvent>>;
    /// Delete the metadata row for a service
    async fn delete_meta(&self, service_id: &str) -> Result<()>;
}

/// Minimal summary for listing services
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSummary {
    pub id: String,
    pub name: String,
    pub state: schema::ServiceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

struct NoopControlPlane;

#[async_trait::async_trait]
impl ControlPlane for NoopControlPlane {
    async fn list(&self) -> Result<Vec<ServiceSummary>> {
        Ok(vec![])
    }
    async fn start(
        &self,
        _service_id: &str,
        _port: Option<u16>,
        _hostname: Option<String>,
    ) -> Result<()> {
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
    ) -> Result<mpsc::Receiver<ServiceEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn delete_meta(&self, _service_id: &str) -> Result<()> {
        Err(IpcError::ProtocolError("deleteMeta not implemented".into()))
    }
}

#[cfg(feature = "supervisor")]
#[allow(missing_docs)]
pub mod supervisor_adapter {
    use super::{ControlPlane, Result, ServiceDetail, ServiceSummary};
    use async_trait::async_trait;
    use canopus_core::supervisor::SupervisorHandle;
    use canopus_core::PortGuard;
    use schema::ServiceEvent;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{broadcast, mpsc};

    // Reduce type complexity for runtime metadata storage
    #[derive(Default)]
    struct RuntimeMetaEntry {
        port: Option<u16>,
        hostname: Option<String>,
        port_guard: Option<PortGuard>,
    }

    type RuntimeMeta = HashMap<String, RuntimeMetaEntry>;

    struct PortGuardRelease<'a> {
        runtime_meta: &'a std::sync::Mutex<RuntimeMeta>,
        service_id: String,
        active: bool,
    }

    impl<'a> PortGuardRelease<'a> {
        fn inactive(runtime_meta: &'a std::sync::Mutex<RuntimeMeta>, service_id: &str) -> Self {
            Self {
                runtime_meta,
                service_id: service_id.to_string(),
                active: false,
            }
        }

        const fn activate(&mut self) {
            self.active = true;
        }
    }

    impl Drop for PortGuardRelease<'_> {
        fn drop(&mut self) {
            if !self.active {
                return;
            }
            if let Ok(mut map) = self.runtime_meta.lock() {
                if let Some(entry) = map.get_mut(&self.service_id) {
                    entry.port_guard.take();
                }
            }
        }
    }

    /// Control plane adapter backed by a set of `SupervisorHandle`s and a shared event bus
    #[allow(missing_debug_implementations)]
    pub struct SupervisorControlPlane {
        handles: HashMap<String, SupervisorHandle>,
        event_tx: broadcast::Sender<ServiceEvent>,
        meta: Option<Arc<dyn super::ServiceMetaStore>>,
        // Volatile in-memory metadata to reflect values immediately during this process lifetime
        runtime_meta: std::sync::Mutex<RuntimeMeta>,
        // Login shell PATH captured at bootstrap to allow commands like 'npm'
        login_path: Option<String>,
    }

    impl SupervisorControlPlane {
        #[must_use]
        pub fn new(
            handles: HashMap<String, SupervisorHandle>,
            event_tx: broadcast::Sender<ServiceEvent>,
        ) -> Self {
            Self {
                handles,
                event_tx,
                meta: None,
                runtime_meta: std::sync::Mutex::new(HashMap::new()),
                login_path: None,
            }
        }

        #[must_use]
        pub fn with_meta_store(mut self, meta: Arc<dyn super::ServiceMetaStore>) -> Self {
            self.meta = Some(meta);
            self
        }

        /// Provide a precomputed login PATH to inject into service environments when
        /// they don't explicitly set PATH. This helps resolve commands like `npm`.
        #[must_use]
        pub fn with_login_path(mut self, path: Option<String>) -> Self {
            self.login_path = path;
            self
        }
    }

    #[async_trait]
    impl ControlPlane for SupervisorControlPlane {
        async fn list(&self) -> Result<Vec<ServiceSummary>> {
            let mut out = Vec::with_capacity(self.handles.len());
            for (id, h) in &self.handles {
                let pid = h
                    .get_pid()
                    .await
                    .map_err(|e| super::IpcError::ProtocolError(e.to_string()))?;
                let (mut port, mut hostname) = if let Some(meta) = &self.meta {
                    let p = match meta.get_port(id).await {
                        Ok(port) => port,
                        Err(e) => {
                            tracing::debug!("Failed to fetch port for {}: {}", id, e);
                            None
                        }
                    };
                    let hn = match meta.get_hostname(id).await {
                        Ok(hostname) => hostname,
                        Err(e) => {
                            tracing::debug!("Failed to fetch hostname for {}: {}", id, e);
                            None
                        }
                    };
                    (p, hn)
                } else {
                    (None, None)
                };

                // Use volatile runtime metadata if persistent store returned None
                if port.is_none() || hostname.is_none() {
                    if let Ok(map) = self.runtime_meta.lock() {
                        if let Some(entry) = map.get(id) {
                            if port.is_none() {
                                port = entry.port;
                            }
                            if hostname.is_none() {
                                hostname.clone_from(&entry.hostname);
                            }
                        }
                    }
                }

                // Fallbacks from the spec if metadata isn't yet persisted/visible
                if port.is_none() {
                    if let Ok(spec) = h.get_spec().await {
                        if let Some(pstr) = spec.environment.get("PORT") {
                            if let Ok(pval) = pstr.parse::<u16>() {
                                port = Some(pval);
                            }
                        }
                    }
                }
                if hostname.is_none() {
                    if let Ok(spec) = h.get_spec().await {
                        if let Some(route) = &spec.route {
                            if !route.is_empty() {
                                hostname = Some(route.clone());
                            }
                        }
                    }
                }
                out.push(ServiceSummary {
                    id: id.clone(),
                    name: h.spec.name.clone(),
                    state: h.current_state(),
                    pid,
                    port,
                    hostname,
                });
            }
            Ok(out)
        }

        #[allow(clippy::too_many_lines)]
        async fn start(
            &self,
            service_id: &str,
            port: Option<u16>,
            hostname: Option<String>,
        ) -> Result<()> {
            let handle = self
                .handles
                .get(service_id)
                .ok_or_else(|| super::IpcError::ProtocolError("unknown service".into()))?;

            // Determine port to use (allocate only when missing from spec/env)
            let mut reserved_guard: Option<PortGuard> = None;
            let existing_port = handle
                .spec
                .readiness_check
                .as_ref()
                .and_then(|rc| match rc.check_type {
                    schema::HealthCheckType::Tcp { port } => Some(port),
                    schema::HealthCheckType::Exec { .. } => None,
                })
                .or_else(|| {
                    handle
                        .spec
                        .environment
                        .get("PORT")
                        .and_then(|p| p.parse::<u16>().ok())
                });
            let chosen_port = if let Some(port) = port {
                Some(port)
            } else if let Some(port) = existing_port {
                Some(port)
            } else {
                let alloc = canopus_core::PortAllocator::new();
                let guard = alloc
                    .reserve(None)
                    .map_err(|e| super::IpcError::ProtocolError(e.to_string()))?;
                let port = guard.port();
                reserved_guard = Some(guard);
                Some(port)
            };
            let mut port_guard_release = PortGuardRelease::inactive(&self.runtime_meta, service_id);

            // Possibly update spec with port/hostname and ensure PATH if configured
            {
                let mut spec = handle.spec.clone();
                let should_inject_path = self
                    .login_path
                    .as_ref()
                    .map_or(false, |_| !spec.environment.contains_key("PATH"));
                let need_update = hostname.is_some() || chosen_port.is_some() || should_inject_path;
                if let Some(hn) = hostname.clone() {
                    // Use route field to carry hostname for proxy integration
                    spec.route = Some(hn);
                }
                if let Some(p) = chosen_port {
                    spec.environment.insert("PORT".to_string(), p.to_string());
                    // If readiness check is TCP, align port
                    if let Some(rc) = &mut spec.readiness_check {
                        if let schema::HealthCheckType::Tcp { port: rp } = &mut rc.check_type {
                            *rp = p;
                        }
                    }
                }

                // Inject login PATH if available and not explicitly set in the spec
                if let Some(lp) = &self.login_path {
                    if should_inject_path {
                        spec.environment.insert("PATH".to_string(), lp.clone());
                    }
                }

                if need_update {
                    handle
                        .update_spec(spec)
                        .map_err(|e| super::IpcError::ProtocolError(e.to_string()))?;

                    // Important: wait until the supervisor task applies the spec update
                    // to avoid racing Start with the previous spec (missing PORT/hostname/PATH).
                    let _ = handle
                        .get_spec()
                        .await
                        .map_err(|e| super::IpcError::ProtocolError(e.to_string()))?;
                }
            }

            // Persist metadata if store present
            if let Some(meta) = &self.meta {
                if let Some(p) = chosen_port {
                    let _ = meta.set_port(service_id, Some(p)).await;
                }
                if let Some(hn) = &hostname {
                    let _ = meta.set_hostname(service_id, Some(hn.as_str())).await;
                }
            }

            // Update in-memory runtime metadata immediately
            {
                if let Ok(mut map) = self.runtime_meta.lock() {
                    let entry = map
                        .entry(service_id.to_string())
                        .or_insert_with(RuntimeMetaEntry::default);
                    if chosen_port.is_some() {
                        entry.port = chosen_port;
                    }
                    if let Some(hn) = &hostname {
                        entry.hostname = Some(hn.clone());
                    }
                    if let Some(guard) = reserved_guard.take() {
                        entry.port_guard = Some(guard);
                        port_guard_release.activate();
                    }
                }
            }

            // Persist the effective hostname to the metadata store and update runtime metadata
            // This mirrors the same precedence as above but runs on all platforms.
            {
                let mut persist_hostname = hostname.clone();

                if persist_hostname.is_none() {
                    if let Some(meta) = &self.meta {
                        if let Ok(hn) = meta.get_hostname(service_id).await {
                            if hn.is_some() {
                                persist_hostname = hn;
                            }
                        }
                    }
                }

                if persist_hostname.is_none() {
                    if let Ok(map) = self.runtime_meta.lock() {
                        if let Some(entry) = map.get(service_id) {
                            if let Some(hn) = entry.hostname.clone() {
                                persist_hostname = Some(hn);
                            }
                        }
                    }
                }

                if persist_hostname.is_none() {
                    if let Ok(spec) = handle.get_spec().await {
                        if let Some(route) = spec.route {
                            if !route.is_empty() {
                                persist_hostname = Some(route);
                            }
                        }
                    }
                }

                if let Some(hn) = persist_hostname {
                    if let Some(meta) = &self.meta {
                        let _ = meta.set_hostname(service_id, Some(hn.as_str())).await;
                    }
                    if let Ok(mut map) = self.runtime_meta.lock() {
                        let entry = map
                            .entry(service_id.to_string())
                            .or_insert_with(RuntimeMetaEntry::default);
                        entry.hostname = Some(hn);
                    }
                }
            }

            // Subscribe to state changes before starting to avoid missing Ready transition
            let mut state_rx = handle.subscribe_to_state();

            handle
                .start()
                .map_err(|e| super::IpcError::ProtocolError(e.to_string()))?;

            // Default wait-ready behavior: wait until service reaches Ready state
            let current = *state_rx.borrow();
            let result = if current == schema::ServiceState::Ready {
                Ok(())
            } else {
                let timeout_secs = handle.spec.startup_timeout_secs.saturating_add(5);
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

                match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), wait).await
                {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(())) => Err(super::IpcError::ProtocolError(
                        "service state channel closed".into(),
                    )),
                    Err(_) => Err(super::IpcError::ProtocolError(
                        "timed out waiting for service readiness".into(),
                    )),
                }
            };
            result
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

            let (mut port, mut hostname) = if let Some(meta) = &self.meta {
                let p = meta.get_port(service_id).await.unwrap_or(None);
                let hn = meta.get_hostname(service_id).await.unwrap_or(None);
                (p, hn)
            } else {
                (None, None)
            };

            // Use volatile runtime metadata if persistent store returned None
            if port.is_none() || hostname.is_none() {
                if let Ok(map) = self.runtime_meta.lock() {
                    if let Some(entry) = map.get(service_id) {
                        if port.is_none() {
                            port = entry.port;
                        }
                        if hostname.is_none() {
                            hostname.clone_from(&entry.hostname);
                        }
                    }
                }
            }

            // Fallbacks from the spec if metadata isn't yet persisted/visible
            if port.is_none() {
                if let Ok(spec) = handle.get_spec().await {
                    if let Some(pstr) = spec.environment.get("PORT") {
                        if let Ok(pval) = pstr.parse::<u16>() {
                            port = Some(pval);
                        }
                    }
                }
            }
            if hostname.is_none() {
                if let Ok(spec) = handle.get_spec().await {
                    if let Some(route) = &spec.route {
                        if !route.is_empty() {
                            hostname = Some(route.clone());
                        }
                    }
                }
            }

            Ok(ServiceDetail {
                id: service_id.to_string(),
                name: handle.spec.name.clone(),
                state: handle.current_state(),
                pid,
                port,
                hostname,
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

        async fn assign_port(&self, service_id: &str, preferred: Option<u16>) -> Result<u16> {
            // Allocate a free port best-effort
            let alloc = canopus_core::PortAllocator::new();
            let guard = alloc.reserve(preferred).map_err(|e| {
                super::IpcError::ProtocolError(format!(
                    "assignPort failed for {service_id} (preferred={preferred:?}): {e}"
                ))
            })?;
            Ok(guard.port())
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
                            // Skip lagged events.
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            Ok(out_rx)
        }

        async fn delete_meta(&self, service_id: &str) -> Result<()> {
            if let Some(meta) = &self.meta {
                meta.delete(service_id).await?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_route_method_rejects_invalid_start_port() {
        #[cfg(unix)]
        {
            let (stream, _peer) = tokio::net::UnixStream::pair().expect("unix pair");
            let (_reader, writer) = stream.into_split();
            let writer = Arc::new(Mutex::new(writer));
            let req = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: "canopus.start".to_string(),
                params: Some(serde_json::json!({"serviceId": "svc", "port": 70_000})),
                id: Some(Value::from(1)),
            };
            let resp = route_method(
                &IpcServerConfig::default(),
                Arc::new(NoopControlPlane),
                writer,
                req,
            )
            .await
            .expect("route method")
            .expect("response");
            let err = resp.error.expect("error");
            assert_eq!(err.code, -32602);
            assert!(err.message.contains("port"));
        }
    }

    #[tokio::test]
    async fn test_route_method_rejects_invalid_assign_port() {
        #[cfg(unix)]
        {
            let (stream, _peer) = tokio::net::UnixStream::pair().expect("unix pair");
            let (_reader, writer) = stream.into_split();
            let writer = Arc::new(Mutex::new(writer));
            let req = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: "canopus.assignPort".to_string(),
                params: Some(serde_json::json!({"serviceId": "svc", "preferred": 70_000})),
                id: Some(Value::from(2)),
            };
            let resp = route_method(
                &IpcServerConfig::default(),
                Arc::new(NoopControlPlane),
                writer,
                req,
            )
            .await
            .expect("route method")
            .expect("response");
            let err = resp.error.expect("error");
            assert_eq!(err.code, -32602);
            assert!(err.message.contains("preferred"));
        }
    }

    #[tokio::test]
    async fn test_unix_server_handshake_rejects_without_token_when_required() {
        #[cfg(unix)]
        {
            use tokio::io::AsyncBufReadExt;
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
            let mut data = serde_json::to_vec(&req).unwrap();
            data.push(b'\n');
            stream.write_all(&data).await.unwrap();

            let mut reader = BufReader::new(stream);
            let mut buf = Vec::new();
            let n = reader.read_until(b'\n', &mut buf).await.unwrap();
            assert!(n > 0);
            if matches!(buf.last(), Some(b'\n')) {
                buf.pop();
                if matches!(buf.last(), Some(b'\r')) {
                    buf.pop();
                }
            }
            let v: Value = serde_json::from_slice(&buf).unwrap();
            assert!(v.get("error").is_some());
        }
    }

    #[tokio::test]
    async fn test_read_request_normal() {
        use tokio::io::AsyncWriteExt;
        let (raw_reader, mut writer) = tokio::io::duplex(1024);
        let mut reader = BufReader::new(raw_reader);

        // Send a valid JSON-RPC request
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "canopus.version",
            "id": 1
        });
        let data = format!("{req}\n");
        writer.write_all(data.as_bytes()).await.unwrap();

        // Read and verify
        let result = read_request(&mut reader).await;
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.jsonrpc, "2.0");
        assert_eq!(parsed.method, "canopus.version");
    }

    #[tokio::test]
    async fn test_read_request_exceeds_max_size() {
        use tokio::io::AsyncWriteExt;
        let (raw_reader, mut writer) = tokio::io::duplex(MAX_FRAME_SIZE + 1024);
        let mut reader = BufReader::new(raw_reader);

        // Create a request that exceeds MAX_FRAME_SIZE
        let large_value = "x".repeat(MAX_FRAME_SIZE + 1);
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"test","id":1,"params":{{"large":"{large_value}"}}}}"#
        );
        let data = format!("{req}\n");

        // Send the oversized request
        writer.write_all(data.as_bytes()).await.unwrap();

        // Read should fail with ProtocolError
        let result = read_request(&mut reader).await;
        assert!(result.is_err());
        match result {
            Err(IpcError::ProtocolError(msg)) => {
                assert!(msg.contains("exceeds maximum allowed size"));
            }
            _ => panic!("Expected ProtocolError for oversized frame"),
        }
    }

    #[tokio::test]
    async fn test_read_request_empty() {
        let (raw_reader, writer) = tokio::io::duplex(1024);
        let mut reader = BufReader::new(raw_reader);

        // Close writer to simulate empty input
        drop(writer);

        // Read should return EmptyResponse
        let result = read_request(&mut reader).await;
        assert!(matches!(result, Err(IpcError::EmptyResponse)));
    }
}
