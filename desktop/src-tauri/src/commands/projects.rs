#![allow(clippy::unreachable)]

use tauri::State;

use super::CommandError;
use crate::state::{AppState, ProjectConfig};

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<ProjectConfig, CommandError> {
    match tokio::fs::read_to_string(&state.projects_path).await {
        Ok(content) => serde_json::from_str(&content).map_err(CommandError::from),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectConfig::default()),
        Err(e) => Err(CommandError::from(e)),
    }
}

fn validate_project_names(projects: &[crate::state::Project]) -> Result<(), CommandError> {
    for project in projects {
        let name = project.name.trim();
        if name == "__none__" || name == "__new__" {
            return Err(CommandError {
                code: "PROJ004",
                message: format!("Reserved project name '{name}' is not allowed"),
            });
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn save_projects(
    state: State<'_, AppState>,
    config: ProjectConfig,
) -> Result<(), CommandError> {
    validate_project_names(&config.projects)?;

    let content = serde_json::to_string_pretty(&config).map_err(CommandError::from)?;
    if let Some(parent) = state.projects_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(CommandError::from)?;
    }
    // Atomic write: write to a unique temp file then rename to avoid partial writes.
    // Use timestamp + random suffix to prevent collisions from concurrent saves.
    let tmp_path = state.projects_path.with_extension(format!(
        "tmp.{}.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        rand::random::<u32>()
    ));
    let result = async {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(CommandError::from)?;
        file.write_all(content.as_bytes())
            .await
            .map_err(CommandError::from)?;
        file.flush().await.map_err(CommandError::from)?;
        file.sync_all().await.map_err(CommandError::from)?;
        // On Windows `rename` fails when the destination already exists; the
        // first save succeeds but every subsequent save would return an I/O
        // error.  Remove the destination first so the rename always succeeds.
        // This is not fully atomic on Windows (a crash between the two calls
        // can leave no file at all), but it is the only portable approach
        // without pulling in a Windows-specific atomic-replace helper.
        // On Unix, `rename` atomically replaces so this branch is a no-op.
        #[cfg(windows)]
        let _ = tokio::fs::remove_file(&state.projects_path).await;
        tokio::fs::rename(&tmp_path, &state.projects_path)
            .await
            .map_err(CommandError::from)
    }
    .await;
    if result.is_err() {
        // Best-effort cleanup: remove the temp file to avoid leaving stale data.
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }
    result
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::state::Project;

    fn project(name: &str) -> Project {
        Project {
            name: name.to_string(),
            service_ids: vec![],
        }
    }

    #[test]
    fn proj004_none_is_reserved() {
        let err = validate_project_names(&[project("__none__")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
        assert!(err.message.contains("__none__"));
    }

    #[test]
    fn proj004_new_is_reserved() {
        let err = validate_project_names(&[project("__new__")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
        assert!(err.message.contains("__new__"));
    }

    #[test]
    fn proj004_trimmed_spaces_detected() {
        let err = validate_project_names(&[project("  __none__  ")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
    }

    #[test]
    fn proj004_valid_name_accepted() {
        validate_project_names(&[project("my-project")]).expect("valid name must pass");
    }

    #[test]
    fn proj004_second_project_reserved() {
        let err =
            validate_project_names(&[project("valid"), project("__new__")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
    }
}
