//! UDS JSON-RPC client for local control plane
//!
//! Provides a simple client that connects to the UDS server, performs a handshake
//! (with optional bearer token), and exposes typed methods.

use crate::{IpcError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Mutex};

use crate::server::ServiceSummary;

#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct JsonRpcClient {
    socket_path: PathBuf,
    token: Option<String>,
}

#[allow(missing_docs)]
impl JsonRpcClient {
    pub fn new(socket_path: impl Into<PathBuf>, token: Option<String>) -> Self {
        Self { socket_path: socket_path.into(), token }
    }

    pub async fn version(&self) -> Result<String> {
        let (mut reader, mut writer) = self.connect_and_handshake().await?;
        let req = jsonrpc_req("canopus.version", Value::Null, 1);
        write_json(&mut writer, &req).await?;
        let resp = read_json::<JsonRpcResponse, _>(&mut reader).await?;
        if let Some(result) = resp.result {
            Ok(result.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string())
        } else {
            Err(IpcError::ProtocolError("version call failed".into()))
        }
    }

    pub async fn list(&self) -> Result<Vec<ServiceSummary>> {
        let (mut reader, mut writer) = self.connect_and_handshake().await?;
        let req = jsonrpc_req("canopus.list", Value::Null, 2);
        write_json(&mut writer, &req).await?;
        let resp = read_json::<JsonRpcResponse, _>(&mut reader).await?;
        if let Some(result) = resp.result {
            let services = result.get("services").cloned().unwrap_or(Value::Array(vec![]));
            serde_json::from_value::<Vec<ServiceSummary>>(services)
                .map_err(|e| IpcError::DeserializationFailed(e.to_string()))
        } else {
            Err(IpcError::ProtocolError("list call failed".into()))
        }
    }

    pub async fn start(&self, service_id: &str) -> Result<()> {
        self.simple_ok("canopus.start", serde_json::json!({"serviceId": service_id}), 3).await
    }

    pub async fn stop(&self, service_id: &str) -> Result<()> {
        self.simple_ok("canopus.stop", serde_json::json!({"serviceId": service_id}), 4).await
    }

    pub async fn restart(&self, service_id: &str) -> Result<()> {
        self.simple_ok("canopus.restart", serde_json::json!({"serviceId": service_id}), 5).await
    }

    pub async fn health_check(&self, service_id: &str) -> Result<bool> {
        let (mut reader, mut writer) = self.connect_and_handshake().await?;
        let req = jsonrpc_req("canopus.healthCheck", serde_json::json!({"serviceId": service_id}), 6);
        write_json(&mut writer, &req).await?;
        let resp = read_json::<JsonRpcResponse, _>(&mut reader).await?;
        if let Some(result) = resp.result {
            Ok(result.get("healthy").and_then(|v| v.as_bool()).unwrap_or(false))
        } else {
            Err(IpcError::ProtocolError("healthCheck call failed".into()))
        }
    }

    pub async fn bind_host(&self, service_id: &str, host: &str) -> Result<()> {
        let (mut reader, mut writer) = self.connect_and_handshake().await?;
        let req = jsonrpc_req(
            "canopus.bindHost",
            serde_json::json!({"serviceId": service_id, "host": host}),
            7,
        );
        write_json(&mut writer, &req).await?;
        let resp = read_json::<JsonRpcResponse, _>(&mut reader).await?;
        if resp.error.is_some() { return Err(IpcError::ProtocolError("bindHost failed".into())); }
        Ok(())
    }

    pub async fn assign_port(&self, service_id: &str, preferred: Option<u16>) -> Result<u16> {
        let (mut reader, mut writer) = self.connect_and_handshake().await?;
        let req = jsonrpc_req(
            "canopus.assignPort",
            serde_json::json!({"serviceId": service_id, "preferred": preferred}),
            8,
        );
        write_json(&mut writer, &req).await?;
        let resp = read_json::<JsonRpcResponse, _>(&mut reader).await?;
        if let Some(result) = resp.result {
            Ok(result.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16)
        } else {
            Err(IpcError::ProtocolError("assignPort failed".into()))
        }
    }

    pub async fn tail_logs(&self, service_id: &str, from_seq: Option<u64>) -> Result<mpsc::Receiver<schema::ServiceEvent>> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(|e| IpcError::ConnectionFailed(e.to_string()))?;
        let (mut reader, writer) = stream.into_split();
        let writer = Arc::new(Mutex::new(writer));

        // Handshake
        let hs = jsonrpc_req(
            "canopus.handshake",
            match &self.token {
                Some(t) => serde_json::json!({"token": t}),
                None => Value::Null,
            },
            100,
        );
        write_json_locked(writer.clone(), &hs).await?;
        let _ = read_json::<JsonRpcResponse, _>(&mut reader).await?;

        // Subscribe
        let sub = jsonrpc_req(
            "canopus.tailLogs",
            serde_json::json!({"serviceId": service_id, "fromSeq": from_seq}),
            101,
        );
        write_json_locked(writer.clone(), &sub).await?;
        let _ = read_json::<JsonRpcResponse, _>(&mut reader).await?; // ack

        // Channel for events
        let (tx, rx) = mpsc::channel(100);
        tokio::spawn(async move {
            while let Ok(v) = read_value(&mut reader).await {
                if let Some(method) = v.get("method").and_then(|m| m.as_str()) {
                    if method == "canopus.tailLogs.update" {
                        if let Some(params) = v.get("params") {
                            if let Ok(evt) = serde_json::from_value::<schema::ServiceEvent>(params.clone()) {
                                let _ = tx.send(evt).await;
                            }
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn simple_ok(&self, method: &str, params: Value, id: u64) -> Result<()> {
        let (mut reader, mut writer) = self.connect_and_handshake().await?;
        let req = jsonrpc_req(method, params, id);
        write_json(&mut writer, &req).await?;
        let resp = read_json::<JsonRpcResponse, _>(&mut reader).await?;
        if resp.error.is_some() {
            return Err(IpcError::ProtocolError(format!("{} failed", method)));
        }
        Ok(())
    }

    async fn connect_and_handshake(&self) -> Result<(tokio::net::unix::OwnedReadHalf, tokio::net::unix::OwnedWriteHalf)> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(|e| IpcError::ConnectionFailed(e.to_string()))?;
        let (mut reader, mut writer) = stream.into_split();

        let req = jsonrpc_req(
            "canopus.handshake",
            match &self.token {
                Some(t) => serde_json::json!({"token": t}),
                None => Value::Null,
            },
            0,
        );
        write_json(&mut writer, &req).await?;
        let _resp: JsonRpcResponse = read_json(&mut reader).await?;
        Ok((reader, writer))
    }
}

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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<crate::server::JsonRpcError>,
    #[serde(default)]
    id: Option<Value>,
}

fn jsonrpc_req(method: &str, params: Value, id: u64) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params: if params.is_null() { None } else { Some(params) },
        id: Some(Value::from(id)),
    }
}

async fn write_json<S: AsyncWriteExt + Unpin>(writer: &mut S, v: &impl Serialize) -> Result<()> {
    let data = serde_json::to_vec(v).map_err(|e| IpcError::SerializationFailed(e.to_string()))?;
    writer.write_all(&data).await.map_err(|e| IpcError::SendFailed(e.to_string()))
}

async fn write_json_locked(writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>, v: &impl Serialize) -> Result<()> {
    let data = serde_json::to_vec(v).map_err(|e| IpcError::SerializationFailed(e.to_string()))?;
    let mut guard = writer.lock().await;
    guard.write_all(&data).await.map_err(|e| IpcError::SendFailed(e.to_string()))
}

async fn read_json<T: for<'de> Deserialize<'de>, S: AsyncReadExt + Unpin>(reader: &mut S) -> Result<T> {
    let mut buf = vec![0u8; 65536];
    let n = reader.read(&mut buf).await.map_err(|e| IpcError::ReceiveFailed(e.to_string()))?;
    if n == 0 { return Err(IpcError::EmptyResponse); }
    serde_json::from_slice(&buf[..n]).map_err(|e| IpcError::DeserializationFailed(e.to_string()))
}

async fn read_value<S: AsyncReadExt + Unpin>(reader: &mut S) -> Result<Value> {
    let mut buf = vec![0u8; 65536];
    let n = reader.read(&mut buf).await.map_err(|e| IpcError::ReceiveFailed(e.to_string()))?;
    if n == 0 { return Err(IpcError::EmptyResponse); }
    serde_json::from_slice(&buf[..n]).map_err(|e| IpcError::DeserializationFailed(e.to_string()))
}
