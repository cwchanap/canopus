mod commands;
mod state;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = AppState::new().map_err(|e| e.to_string())?;
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
        .expect("error while running tauri application");
}
