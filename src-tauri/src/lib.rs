pub mod commands;
pub mod discovery;
pub mod model;
pub mod parsers;
pub mod pricing;

use std::sync::RwLock;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let cache_dir = app.path().app_cache_dir()?;
            app.manage(commands::AppState {
                pricing: RwLock::new(pricing::PriceTable::load(&cache_dir)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::get_session,
            commands::get_settings,
            commands::save_settings,
            commands::refresh_pricing,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
