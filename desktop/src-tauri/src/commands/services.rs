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

/// Payload emitted for each log line received.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEventPayload {
    pub service_id: String,
    pub stream: String,
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
    // Acquire the lock only to remove any existing handle, then drop it before
    // awaiting tail_logs so we don't hold the mutex across an await point.
    let old_handle = {
        let mut tails = state.log_tails.lock().await;
        tails.remove(&service_id)
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
    let handle = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let schema::ServiceEvent::LogOutput {
                stream,
                content,
                timestamp,
                ..
            } = event
            {
                // Use the serde representation for a stable string (e.g. "stdout",
                // "stderr") rather than the Debug format, which would break if
                // the enum's Debug implementation or variant names ever change.
                let stream_str = serde_json::to_value(&stream)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown".to_owned());
                let payload = LogEventPayload {
                    service_id: svc_id.clone(),
                    stream: stream_str,
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
        // The log stream has ended (sender dropped or daemon closed the stream).
        // Notify the frontend so it can update the UI, and remove the now-completed
        // handle from log_tails to avoid accumulating stale entries.
        let _ = app.emit("log-tail-ended", &svc_id);
        if let Some(state) = app.try_state::<AppState>() {
            state.log_tails.lock().await.remove(&svc_id);
        }
    });

    state.log_tails.lock().await.insert(service_id, handle);
    Ok(())
}

/// Stop tailing logs for a service.
#[tauri::command]
pub async fn stop_log_tail(
    state: State<'_, AppState>,
    service_id: String,
) -> Result<(), CommandError> {
    let mut tails = state.log_tails.lock().await;
    if let Some(handle) = tails.remove(&service_id) {
        handle.abort();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_event_payload_serializes_correctly() {
        let payload = LogEventPayload {
            service_id: "my-service".to_string(),
            stream: "stdout".to_string(),
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
            stream: "stderr".to_string(),
            content: "error line".to_string(),
            timestamp: "2026-01-01T00:00:01Z".to_string(),
        };
        let json = serde_json::to_string(&payload).expect("serialization failed");
        assert!(json.contains("\"stream\":\"stderr\""));
        assert!(json.contains("\"content\":\"error line\""));
    }
}
