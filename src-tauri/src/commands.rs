use crate::aggregate::{self, SessionGroup};
use crate::db::{Store, SyncStats};
use crate::discovery::{self, Settings, SettingsView};
use crate::model::{SessionDetail, SessionSummary, Source};
use crate::parsers::{claude, codex};
use crate::pricing::{PriceTable, PricingStatus};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, RwLock};
use tauri::{Manager, State};

/// Group map key. `Source` is not `Hash`, so use its stable name.
type GroupKey = (&'static str, String);

fn group_key(source: Source, id: &str) -> GroupKey {
    let src = match source {
        Source::Claude => "claude",
        Source::Codex => "codex",
    };
    (src, id.to_string())
}

pub struct AppState {
    pub pricing: RwLock<PriceTable>,
    pub store: Mutex<Store>,
    /// Groups from the last list run, for detail lookups.
    pub groups: Mutex<HashMap<GroupKey, SessionGroup>>,
}

impl AppState {
    pub fn new(pricing: PriceTable, store: Store) -> Self {
        Self {
            pricing: RwLock::new(pricing),
            store: Mutex::new(store),
            groups: Mutex::new(HashMap::new()),
        }
    }
}

fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

fn cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_cache_dir().map_err(|e| e.to_string())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn settings_view(app: &tauri::AppHandle, state: &AppState) -> Result<SettingsView, String> {
    let settings = discovery::load_settings(&config_dir(app)?);
    let (c, x) = discovery::effective_dirs(&settings);
    let strs = |v: Vec<PathBuf>| v.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let pricing = state.pricing.read().map_err(|e| e.to_string())?.status();
    Ok(SettingsView {
        settings,
        claude_dirs: strs(c),
        codex_dirs: strs(x),
        pricing,
    })
}

fn parse_any(source: Source, path: &Path) -> anyhow::Result<crate::model::ParsedSession> {
    match source {
        Source::Claude => claude::parse_file(path),
        Source::Codex => codex::parse_file(path),
    }
}

/// sync -> load_all -> build. Pure over its inputs (no Tauri state).
fn run_pipeline(
    store: &mut Store,
    claude_files: &[PathBuf],
    codex_files: &[PathBuf],
    prices: &PriceTable,
) -> anyhow::Result<(SyncStats, Vec<SessionGroup>, Vec<SessionSummary>)> {
    let stats = store.sync(claude_files, codex_files, parse_any)?;
    let sessions = store.load_all()?;
    let (groups, summaries) = aggregate::build(sessions, prices);
    Ok((stats, groups, summaries))
}

/// Sync the DB with disk, rebuild groups in state, return summaries.
async fn refresh(app: &tauri::AppHandle) -> Result<Vec<SessionSummary>, String> {
    let settings = discovery::load_settings(&config_dir(app)?);
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (cd, xd) = discovery::effective_dirs(&settings);
        let cf = discovery::find_jsonl(&cd);
        let xf = discovery::find_jsonl(&xd);
        let (_, groups, summaries) = {
            let prices = state.pricing.read().map_err(|e| e.to_string())?;
            run_pipeline(&mut lock(&state.store), &cf, &xf, &prices).map_err(|e| e.to_string())?
        };
        let map: HashMap<GroupKey, SessionGroup> = groups
            .into_iter()
            .map(|g| (group_key(g.source, &g.id), g))
            .collect();
        *lock(&state.groups) = map;
        Ok(summaries)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_sessions(app: tauri::AppHandle) -> Result<Vec<SessionSummary>, String> {
    refresh(&app).await
}

#[tauri::command]
pub async fn get_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    source: Source,
    id: String,
) -> Result<SessionDetail, String> {
    let key = group_key(source, &id);
    let detail = |state: &AppState| -> Result<Option<SessionDetail>, String> {
        let groups = lock(&state.groups);
        let Some(g) = groups.get(&key) else {
            return Ok(None);
        };
        let prices = state.pricing.read().map_err(|e| e.to_string())?;
        Ok(Some(aggregate::detail_group(g, &prices)))
    };
    if let Some(d) = detail(&state)? {
        return Ok(d);
    }
    refresh(&app).await?;
    detail(&state)?.ok_or_else(|| "session not found".to_string())
}

#[tauri::command]
pub fn get_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<SettingsView, String> {
    settings_view(&app, &state)
}

#[tauri::command]
pub fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<SettingsView, String> {
    discovery::save_settings(&config_dir(&app)?, &settings).map_err(|e| e.to_string())?;
    settings_view(&app, &state)
}

#[tauri::command]
pub async fn refresh_pricing(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<PricingStatus, String> {
    let table = PriceTable::refresh(&cache_dir(&app)?)
        .await
        .map_err(|e| e.to_string())?;
    let status = table.status();
    *state.pricing.write().map_err(|e| e.to_string())? = table;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn lock_recovers_poisoned() {
        let m = Arc::new(Mutex::new(1));
        let m2 = Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert_eq!(*lock(&m), 1);
    }

    #[test]
    fn pipeline_second_run_parses_nothing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cf = discovery::find_jsonl(&[root.join("claude")]);
        let xf = discovery::find_jsonl(&[root.join("codex")]);
        assert!(!cf.is_empty() && !xf.is_empty());
        let prices = PriceTable::load(Path::new("/nonexistent-cache-dir"));
        let mut store = Store::open_in_memory().unwrap();

        let (s1, groups, sums) = run_pipeline(&mut store, &cf, &xf, &prices).unwrap();
        assert!(s1.parsed > 0);
        assert_eq!(groups.len(), sums.len());
        assert!(groups.iter().any(|g| g.source == Source::Codex));
        assert!(sums
            .iter()
            .any(|s| s.source == Source::Claude && s.subagent_count > 0));

        let (s2, groups2, sums2) = run_pipeline(&mut store, &cf, &xf, &prices).unwrap();
        assert_eq!(s2.parsed, 0);
        assert_eq!(s2.unchanged, s1.parsed);
        assert_eq!(groups2.len(), groups.len());
        assert_eq!(sums2.len(), sums.len());
    }
}
