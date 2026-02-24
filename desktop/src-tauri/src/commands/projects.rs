use tauri::State;

use crate::state::{AppState, ProjectConfig};

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<ProjectConfig, String> {
    match tokio::fs::read_to_string(&state.projects_path).await {
        Ok(content) => serde_json::from_str(&content).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectConfig::default()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn save_projects(
    state: State<'_, AppState>,
    config: ProjectConfig,
) -> Result<(), String> {
    let content = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    if let Some(parent) = state.projects_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    // Atomic write: write to a temp file then rename to avoid partial writes.
    let tmp_path = state.projects_path.with_extension("tmp");
    {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(|e| e.to_string())?;
        file.write_all(content.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        file.flush().await.map_err(|e| e.to_string())?;
        file.sync_all().await.map_err(|e| e.to_string())?;
    }
    tokio::fs::rename(&tmp_path, &state.projects_path)
        .await
        .map_err(|e| e.to_string())
}
