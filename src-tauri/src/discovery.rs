use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub extra_claude_dirs: Vec<String>,
    pub extra_codex_dirs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub claude_dirs: Vec<String>,
    pub codex_dirs: Vec<String>,
    pub pricing: crate::pricing::PricingStatus,
}

pub fn default_claude_dirs() -> Vec<PathBuf> {
    vec![]
}

pub fn default_codex_dirs() -> Vec<PathBuf> {
    vec![]
}

pub fn find_jsonl(_dirs: &[PathBuf]) -> Vec<PathBuf> {
    vec![]
}

pub fn load_settings(_config_dir: &Path) -> Settings {
    Settings::default()
}

pub fn save_settings(_config_dir: &Path, _s: &Settings) -> anyhow::Result<()> {
    Ok(())
}
