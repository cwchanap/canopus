#![allow(clippy::unreachable)]

use tauri::State;

use std::collections::HashSet;
use std::path::Path;

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
    let mut seen_names = HashSet::new();

    for project in projects {
        let name = project.name.trim();
        if name.is_empty() {
            return Err(CommandError {
                code: "PROJ004",
                message: "Blank project name is not allowed".to_string(),
            });
        }
        if name == "__none__" || name == "__new__" {
            return Err(CommandError {
                code: "PROJ004",
                message: format!("Reserved project name '{name}' is not allowed"),
            });
        }

        let normalized_name = name.to_lowercase();
        if !seen_names.insert(normalized_name) {
            return Err(CommandError {
                code: "PROJ004",
                message: format!("Duplicate project name '{name}' is not allowed"),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_field_names)]
struct ProjectValidationSummary {
    blank_names: usize,
    reserved_names: usize,
    duplicate_names: usize,
}

impl ProjectValidationSummary {
    #[allow(clippy::arithmetic_side_effects)]
    const fn score(self) -> usize {
        self.blank_names + self.reserved_names + self.duplicate_names
    }

    const fn is_corrective_from(self, existing: Self) -> bool {
        let reserved_ok = self.reserved_names <= existing.reserved_names;
        let blank_ok = self.blank_names <= existing.blank_names;
        let duplicate_ok = self.duplicate_names <= existing.duplicate_names;

        reserved_ok && blank_ok && duplicate_ok && self.score() < existing.score()
    }
}

#[allow(clippy::arithmetic_side_effects)]
fn summarize_project_names(projects: &[crate::state::Project]) -> ProjectValidationSummary {
    let mut seen_names = HashSet::new();
    let mut blank_names = 0;
    let mut reserved_names = 0;
    let mut duplicate_names = 0;

    for project in projects {
        let name = project.name.trim();
        if name.is_empty() {
            blank_names += 1;
            continue;
        }
        if name == "__none__" || name == "__new__" {
            reserved_names += 1;
        }

        let normalized_name = name.to_lowercase();
        if !seen_names.insert(normalized_name) {
            duplicate_names += 1;
        }
    }

    ProjectValidationSummary {
        blank_names,
        reserved_names,
        duplicate_names,
    }
}

async fn load_existing_project_config(path: &Path) -> Result<ProjectConfig, CommandError> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => serde_json::from_str(&content).map_err(CommandError::from),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectConfig::default()),
        Err(e) => Err(CommandError::from(e)),
    }
}

async fn validate_project_save(path: &Path, config: &ProjectConfig) -> Result<(), CommandError> {
    match validate_project_names(&config.projects) {
        Ok(()) => Ok(()),
        Err(err) => {
            let existing = load_existing_project_config(path).await?;
            let existing_summary = summarize_project_names(&existing.projects);
            let new_summary = summarize_project_names(&config.projects);

            if new_summary.is_corrective_from(existing_summary) {
                return Ok(());
            }

            Err(err)
        }
    }
}

#[tauri::command]
pub async fn save_projects(
    state: State<'_, AppState>,
    config: ProjectConfig,
) -> Result<(), CommandError> {
    validate_project_save(&state.projects_path, &config).await?;

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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn project(name: &str) -> Project {
        Project {
            name: name.to_string(),
            service_ids: vec![],
        }
    }

    fn config(names: &[&str]) -> ProjectConfig {
        ProjectConfig {
            projects: names.iter().map(|name| project(name)).collect(),
        }
    }

    async fn with_temp_projects_path<F, Fut>(test: F)
    where
        F: FnOnce(std::path::PathBuf) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        // Use timestamp + random suffix to prevent collisions from concurrent tests.
        // rand::random::<u32>() provides ~4 billion unique values, making collisions
        // effectively impossible even when multiple tests start at the same nanosecond.
        let unique = format!(
            "canopus-projects-{}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos(),
            rand::random::<u32>()
        );
        let path = std::env::temp_dir().join(unique);
        test(path.clone()).await;
        let _ = tokio::fs::remove_file(path).await;
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
    fn proj004_blank_name_rejected() {
        let err = validate_project_names(&[project("   ")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
        assert!(err.message.contains("Blank project name"));
    }

    #[test]
    fn proj004_empty_name_rejected() {
        let err = validate_project_names(&[project("")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
        assert!(err.message.contains("Blank project name"));
    }

    #[test]
    fn proj004_second_project_reserved() {
        let err = validate_project_names(&[project("valid"), project("__new__")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
    }

    #[test]
    fn proj004_exact_duplicate_rejected() {
        let err = validate_project_names(&[project("alpha"), project("alpha")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
        assert!(err.message.contains("Duplicate project name"));
    }

    #[test]
    fn proj004_case_only_duplicate_rejected() {
        let err = validate_project_names(&[project("Alpha"), project(" alpha ")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
        assert!(err.message.contains("alpha"));
    }

    #[test]
    fn summarize_project_names_counts_invalid_entries() {
        let summary = summarize_project_names(&[
            project("   "),
            project("__none__"),
            project("Alpha"),
            project(" alpha "),
        ]);

        assert_eq!(summary.blank_names, 1);
        assert_eq!(summary.reserved_names, 1);
        assert_eq!(summary.duplicate_names, 1);
        assert_eq!(summary.score(), 3);
    }

    #[tokio::test]
    async fn validate_project_save_allows_corrective_save_from_reserved_legacy_config() {
        with_temp_projects_path(|path| async move {
            let legacy = config(&["__none__", "valid"]);
            let repaired = config(&["fixed", "valid"]);

            tokio::fs::write(
                &path,
                serde_json::to_string(&legacy).expect("legacy config should serialize"),
            )
            .await
            .expect("legacy config should be written");

            validate_project_save(&path, &repaired)
                .await
                .expect("repairing reserved project name should be allowed");
        })
        .await;
    }

    #[tokio::test]
    async fn validate_project_save_rejects_non_corrective_save_from_reserved_legacy_config() {
        with_temp_projects_path(|path| async move {
            let legacy = config(&["__none__", "valid"]);
            let still_invalid = config(&["__none__", "valid", "another"]);

            tokio::fs::write(
                &path,
                serde_json::to_string(&legacy).expect("legacy config should serialize"),
            )
            .await
            .expect("legacy config should be written");

            let err = validate_project_save(&path, &still_invalid)
                .await
                .expect_err("save that leaves invalidity unchanged should fail");

            assert_eq!(err.code, "PROJ004");
        })
        .await;
    }

    #[tokio::test]
    async fn validate_project_save_allows_duplicate_repair() {
        with_temp_projects_path(|path| async move {
            let legacy = config(&["Alpha", " alpha "]);
            let repaired = config(&["Alpha", "beta"]);

            tokio::fs::write(
                &path,
                serde_json::to_string(&legacy).expect("legacy config should serialize"),
            )
            .await
            .expect("legacy config should be written");

            validate_project_save(&path, &repaired)
                .await
                .expect("repairing duplicate project names should be allowed");
        })
        .await;
    }

    #[tokio::test]
    async fn validate_project_save_allows_incremental_fix_when_other_error_categories_remain() {
        with_temp_projects_path(|path| async move {
            let legacy = config(&["__none__", "Alpha", " alpha "]);
            let repaired = config(&["__none__", "beta"]);

            tokio::fs::write(
                &path,
                serde_json::to_string(&legacy).expect("legacy config should serialize"),
            )
            .await
            .expect("legacy config should be written");

            validate_project_save(&path, &repaired).await.expect(
                "repair that reduces invalidity without worsening any category should be allowed",
            );
        })
        .await;
    }

    #[tokio::test]
    async fn validate_project_save_allows_partial_fix_that_keeps_other_error_types_unchanged() {
        with_temp_projects_path(|path| async move {
            let legacy = config(&["__none__", "Alpha", " alpha "]);
            let partial_fix = config(&["__new__", "beta"]);

            tokio::fs::write(
                &path,
                serde_json::to_string(&legacy).expect("legacy config should serialize"),
            )
            .await
            .expect("legacy config should be written");

            validate_project_save(&path, &partial_fix)
                .await
                .expect("save that improves one invalidity category without worsening others should be allowed");
        })
        .await;
    }

    #[tokio::test]
    async fn validate_project_save_rejects_fix_that_introduces_new_error_category() {
        with_temp_projects_path(|path| async move {
            let legacy = config(&["Alpha", " alpha "]);
            let introduces_reserved = config(&["__none__", "beta"]);

            tokio::fs::write(
                &path,
                serde_json::to_string(&legacy).expect("legacy config should serialize"),
            )
            .await
            .expect("legacy config should be written");

            let err = validate_project_save(&path, &introduces_reserved)
                .await
                .expect_err("save that introduces reserved name should fail");

            assert_eq!(err.code, "PROJ004");
        })
        .await;
    }
}
