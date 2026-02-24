use tauri::State;

use crate::state::{AppState, ProjectConfig};

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<ProjectConfig, String> {
    if !state.projects_path.exists() {
        return Ok(ProjectConfig::default());
    }
    let content = tokio::fs::read_to_string(&state.projects_path)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
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
    tokio::fs::write(&state.projects_path, content)
        .await
        .map_err(|e| e.to_string())
}
