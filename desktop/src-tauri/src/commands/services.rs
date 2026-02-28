#![allow(clippy::unreachable, clippy::significant_drop_tightening)]

use ipc::server::{ServiceDetail, ServiceSummary};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use super::CommandError;
use crate::state::AppState;

#[tauri::command]
pub async fn list_services(
    state: State<'_, AppState>,
) -> Result<Vec<ServiceSummary>, CommandError> {
    state.ipc.list().await.map_err(CommandError::from)
}

#[tauri::command]
pub async fn get_service_detail(
    state: State<'_, AppState>,
    service_id: String,
) -> Result<ServiceDetail, CommandError> {
    state
        .ipc
        .status(&service_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn start_service(
    state: State<'_, AppState>,
    service_id: String,
    port: Option<u16>,
    hostname: Option<String>,
) -> Result<(), CommandError> {
    state
        .ipc
        .start(&service_id, port, hostname.as_deref())
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn stop_service(
    state: State<'_, AppState>,
    service_id: String,
) -> Result<(), CommandError> {
    state
        .ipc
        .stop(&service_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn restart_service(
    state: State<'_, AppState>,
    service_id: String,
) -> Result<(), CommandError> {
    state
        .ipc
        .restart(&service_id)
        .await
        .map_err(CommandError::from)
}

/// Log stream direction for frontend event payloads.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
}

impl From<schema::LogStream> for LogStream {
    fn from(s: schema::LogStream) -> Self {
        match s {
            schema::LogStream::Stdout => Self::Stdout,
            schema::LogStream::Stderr => Self::Stderr,
        }
    }
}

/// Payload emitted for each log line received.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEventPayload {
    pub service_id: String,
    pub stream: LogStream,
    pub content: String,
    pub timestamp: String,
}

/// Start tailing logs for a service. Emits `log-update` Tauri events.
#[tauri::command]
pub async fn start_log_tail(
    app: AppHandle,
    state: State<'_, AppState>,
    service_id: String,
) -> Result<(), CommandError> {
    // Remove any existing tail and derive the next generation number.  Insert a
    // None placeholder for our generation immediately so stop_log_tail can cancel
    // us while tail_logs is still in-flight; drop the lock before the await.
    let (gen, old_handle) = {
        let mut tails = state.log_tails.lock().await;
        let old = tails.remove(&service_id);
        let next_gen = old.as_ref().map_or(1, |(g, _)| g.saturating_add(1));
        // Placeholder: handle is None until the real JoinHandle exists.
        tails.insert(service_id.clone(), (next_gen, None));
        (next_gen, old.and_then(|(_, h)| h))
    };
    if let Some(handle) = old_handle {
        handle.abort();
    }

    let mut rx = state
        .ipc
        .tail_logs(&service_id, None)
        .await
        .map_err(CommandError::from)?;

    let svc_id = service_id.clone();
    let task_gen = gen;
    let handle = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let schema::ServiceEvent::LogOutput {
                stream,
                content,
                timestamp,
                ..
            } = event
            {
                let payload = LogEventPayload {
                    service_id: svc_id.clone(),
                    stream: LogStream::from(stream),
                    content,
                    timestamp,
                };
                if app.emit("log-update", payload).is_err() {
                    // The app handle is gone (window closed or shutting down).
                    // No point continuing; stop the task to avoid a zombie reader.
                    break;
                }
            }
        }
        // The log stream ended naturally. Notify the frontend and remove our entry
        // from log_tails, but only if the stored generation still matches ours.
        // A concurrent start_log_tail call may have already replaced this entry with
        // a newer generation, in which case we must not evict it.
        let _ = app.emit("log-tail-ended", &svc_id);
        if let Some(state) = app.try_state::<AppState>() {
            let mut tails = state.log_tails.lock().await;
            if let Some((stored_gen, _)) = tails.get(&svc_id) {
                if *stored_gen == task_gen {
                    tails.remove(&svc_id);
                }
            }
        }
    });

    // Re-acquire the lock and decide whether to promote the placeholder.
    let mut tails = state.log_tails.lock().await;
    match tails.get(&service_id).map(|(g, _)| *g) {
        None => {
            // stop_log_tail removed our placeholder while tail_logs was in-flight.
            // Abort the just-spawned task to prevent an orphaned background tail.
            handle.abort();
        }
        Some(stored_gen) if stored_gen > gen => {
            // A concurrent start_log_tail already inserted a newer generation.
            // Leave it in place and discard ours.
            handle.abort();
        }
        Some(stored_gen) if stored_gen < gen => {
            // Defensive: our gen is newer than whatever is there.  Evict and claim.
            if let Some((_, Some(stale))) = tails.remove(&service_id) {
                stale.abort();
            }
            tails.insert(service_id, (gen, Some(handle)));
        }
        Some(_) => {
            // stored_gen == gen: our placeholder is still there.  Promote to a real handle.
            tails.insert(service_id, (gen, Some(handle)));
        }
    }
    Ok(())
}

/// Stop tailing logs for a service.
#[tauri::command]
pub async fn stop_log_tail(
    state: State<'_, AppState>,
    service_id: String,
) -> Result<(), CommandError> {
    let mut tails = state.log_tails.lock().await;
    // Removing the entry also cancels a pending start: if start_log_tail's placeholder
    // is present (handle == None), its post-await check will find no entry and abort.
    if let Some((_, handle)) = tails.remove(&service_id) {
        if let Some(h) = handle {
            h.abort();
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn log_stream_stdout_converts_correctly() {
        let stream = LogStream::from(schema::LogStream::Stdout);
        let json = serde_json::to_string(&stream).expect("must serialize");
        assert_eq!(json, "\"stdout\"");
    }

    #[test]
    fn log_stream_stderr_converts_correctly() {
        let stream = LogStream::from(schema::LogStream::Stderr);
        let json = serde_json::to_string(&stream).expect("must serialize");
        assert_eq!(json, "\"stderr\"");
    }

    #[test]
    fn log_event_payload_serializes_correctly() {
        let payload = LogEventPayload {
            service_id: "my-service".to_string(),
            stream: LogStream::Stdout,
            content: "hello world".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&payload).expect("serialization failed");
        assert!(json.contains("\"serviceId\":\"my-service\""));
        assert!(json.contains("\"stream\":\"stdout\""));
        assert!(json.contains("\"content\":\"hello world\""));
    }

    #[test]
    fn log_event_payload_stderr_variant() {
        let payload = LogEventPayload {
            service_id: "svc".to_string(),
            stream: LogStream::Stderr,
            content: "error line".to_string(),
            timestamp: "2026-01-01T00:00:01Z".to_string(),
        };
        let json = serde_json::to_string(&payload).expect("serialization failed");
        assert!(json.contains("\"stream\":\"stderr\""));
        assert!(json.contains("\"content\":\"error line\""));
    }
}
