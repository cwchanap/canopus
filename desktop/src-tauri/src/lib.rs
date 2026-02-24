mod commands;
mod state;

use state::AppState;
use tauri::Manager;

/// Categorical errors for the desktop crate entry point.
#[derive(Debug)]
pub enum CrateError {
    /// Failed to initialise application state (CORE001).
    AppStateInit(String),
    /// Tauri runtime failed to start (CORE002).
    TauriRun(String),
}

impl std::fmt::Display for CrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppStateInit(e) => write!(f, "CORE001 app-state init failed: {e}"),
            Self::TauriRun(e) => write!(f, "CORE002 tauri run failed: {e}"),
        }
    }
}

impl std::error::Error for CrateError {}

pub type Result<T> = std::result::Result<T, CrateError>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<()> {
    tauri::Builder::default()
        .setup(|app| {
            let state = AppState::new()
                .map_err(|e| CrateError::AppStateInit(e.to_string()))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::services::list_services,
            commands::services::get_service_detail,
            commands::services::start_service,
            commands::services::stop_service,
            commands::services::restart_service,
            commands::services::start_log_tail,
            commands::services::stop_log_tail,
            commands::inbox::list_inbox,
            commands::inbox::mark_inbox_read,
            commands::inbox::dismiss_inbox_item,
            commands::projects::list_projects,
            commands::projects::save_projects,
        ])
        .run(tauri::generate_context!())
        .map_err(|e| CrateError::TauriRun(e.to_string()))
}
