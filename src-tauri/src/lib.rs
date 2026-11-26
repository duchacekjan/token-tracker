pub mod aggregate;
pub mod commands;
pub mod db;
pub mod discovery;
pub mod model;
pub mod parsers;
pub mod pricing;
pub mod startup;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let cache_dir = app.path().app_cache_dir()?;
            let db_path = app.path().app_data_dir()?.join("token-tracker.sqlite");
            let (store, db_warning) = startup::open_store_with_recovery(&db_path);
            app.manage(commands::AppState::new(
                pricing::PriceTable::load(&cache_dir),
                store,
                db_warning,
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::get_session,
            commands::get_sessions_detail,
            commands::save_csv,
            commands::get_settings,
            commands::save_settings,
            commands::refresh_pricing,
            commands::get_db_warning,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
