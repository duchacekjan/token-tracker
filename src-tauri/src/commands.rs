use crate::discovery::{Settings, SettingsView};
use crate::model::{SessionDetail, SessionSummary};
use crate::pricing::{PriceTable, PricingStatus};
use std::sync::RwLock;
use tauri::State;

pub struct AppState {
    pub pricing: RwLock<PriceTable>,
}

#[tauri::command]
pub fn list_sessions(_state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    Ok(vec![])
}

#[tauri::command]
pub fn get_session(
    _state: State<'_, AppState>,
    _file_path: String,
) -> Result<SessionDetail, String> {
    Err("not implemented".into())
}

#[tauri::command]
pub fn get_settings(
    _app: tauri::AppHandle,
    _state: State<'_, AppState>,
) -> Result<SettingsView, String> {
    Err("not implemented".into())
}

#[tauri::command]
pub fn save_settings(
    _app: tauri::AppHandle,
    _settings: Settings,
) -> Result<SettingsView, String> {
    Err("not implemented".into())
}

#[tauri::command]
pub async fn refresh_pricing(
    _app: tauri::AppHandle,
    _state: State<'_, AppState>,
) -> Result<PricingStatus, String> {
    Err("not implemented".into())
}
