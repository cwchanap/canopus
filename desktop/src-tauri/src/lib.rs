//! Tauri desktop entry point for Canopus.
#![allow(unreachable_pub)]

mod commands;
mod state;

use state::AppState;
use tauri::Manager;

/// Categorical errors for the desktop crate entry point.
pub enum CrateError {
    /// Failed to initialise application state (DESKTOP001).
    AppStateInit(Box<dyn std::error::Error + Send + Sync>),
    /// Tauri runtime failed to start (DESKTOP002).
    TauriRun(tauri::Error),
}

impl std::fmt::Debug for CrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppStateInit(e) => write!(f, "CrateError::AppStateInit({e})"),
            Self::TauriRun(e) => write!(f, "CrateError::TauriRun({e:?})"),
        }
    }
}

impl std::fmt::Display for CrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppStateInit(e) => write!(f, "DESKTOP001 app-state init failed: {e}"),
            Self::TauriRun(e) => write!(f, "DESKTOP002 tauri run failed: {e}"),
        }
    }
}

impl std::error::Error for CrateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AppStateInit(e) => Some(e.as_ref()),
            Self::TauriRun(e) => Some(e),
        }
    }
}

impl From<tauri::Error> for CrateError {
    fn from(e: tauri::Error) -> Self {
        Self::TauriRun(e)
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for CrateError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::AppStateInit(e)
    }
}

pub type Result<T> = std::result::Result<T, CrateError>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::exit)]
/// Start the desktop application runtime.
pub fn run() -> Result<()> {
    // Initialise app state before handing control to Tauri so that init errors
    // are captured as CrateError::AppStateInit rather than being swallowed into
    // tauri::Error (which would route them to CrateError::TauriRun instead).
    let state = AppState::new().map_err(CrateError::AppStateInit)?;

    tauri::Builder::default()
        .setup(|app| {
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
        .run(tauri::generate_context!())?;
    Ok(())
}
