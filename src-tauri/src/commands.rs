use crate::aggregate;
use crate::discovery::{self, Settings, SettingsView};
use crate::model::{ParsedSession, SessionDetail, SessionSummary, Source};
use crate::parsers::{claude, codex};
use crate::pricing::{PriceTable, PricingStatus};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;
use tauri::{Manager, State};

type CacheKey = (PathBuf, Option<SystemTime>, u64);
type ParseCache = HashMap<CacheKey, Arc<ParsedSession>>;

pub struct AppState {
    pub pricing: RwLock<PriceTable>,
    /// Deduped sessions from the last scan, keyed by file path.
    pub sessions: Mutex<HashMap<String, ParsedSession>>,
    /// Raw parse results keyed by (path, mtime, len).
    pub parse_cache: Mutex<ParseCache>,
}

impl AppState {
    pub fn new(pricing: PriceTable) -> Self {
        Self {
            pricing: RwLock::new(pricing),
            sessions: Mutex::new(HashMap::new()),
            parse_cache: Mutex::new(HashMap::new()),
        }
    }
}

fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

fn cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_cache_dir().map_err(|e| e.to_string())
}

fn cache_key(path: &Path) -> Option<CacheKey> {
    let m = std::fs::metadata(path).ok()?;
    Some((path.to_path_buf(), m.modified().ok(), m.len()))
}

fn parse_cached(cache: &Mutex<ParseCache>, source: Source, path: &Path) -> Option<ParsedSession> {
    let key = cache_key(path);
    if let Some(k) = &key {
        if let Some(hit) = cache.lock().ok()?.get(k) {
            return Some((**hit).clone());
        }
    }
    let parsed = match source {
        Source::Claude => claude::parse_file(path),
        Source::Codex => codex::parse_file(path),
    }
    .ok()?;
    if let Some(k) = key {
        cache.lock().ok()?.insert(k, Arc::new(parsed.clone()));
    }
    Some(parsed)
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

#[tauri::command]
pub async fn list_sessions(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SessionSummary>, String> {
    let settings = discovery::load_settings(&config_dir(&app)?);
    let app2 = app.clone();
    let sessions = tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (cd, xd) = discovery::effective_dirs(&settings);
        let cf = discovery::find_jsonl(&cd);
        let xf = discovery::find_jsonl(&xd);
        let sessions = aggregate::scan_with(&cf, &xf, |src, p| {
            parse_cached(&state.parse_cache, src, p)
        });
        // Drop cache entries for files that no longer exist.
        if let Ok(mut c) = state.parse_cache.lock() {
            c.retain(|(p, _, _), _| cf.contains(p) || xf.contains(p));
        }
        sessions
    })
    .await
    .map_err(|e| e.to_string())?;

    let mut summaries: Vec<SessionSummary> = {
        let prices = state.pricing.read().map_err(|e| e.to_string())?;
        sessions
            .iter()
            .map(|s| aggregate::summarize(s, &prices))
            .collect()
    };
    summaries.sort_by(|a, b| match (a.started_at, b.started_at) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    let map: HashMap<String, ParsedSession> = sessions
        .into_iter()
        .map(|s| (s.file_path.clone(), s))
        .collect();
    *state.sessions.lock().map_err(|e| e.to_string())? = map;
    Ok(summaries)
}

fn inside_dirs(path: &Path, dirs: &[PathBuf]) -> bool {
    let Ok(canon) = path.canonicalize() else {
        return false;
    };
    dirs.iter()
        .filter_map(|d| d.canonicalize().ok())
        .any(|d| canon.starts_with(d))
}

#[tauri::command]
pub async fn get_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_path: String,
) -> Result<SessionDetail, String> {
    let stored = state
        .sessions
        .lock()
        .map_err(|e| e.to_string())?
        .get(&file_path)
        .cloned();
    let session = match stored {
        Some(s) => s,
        None => {
            let settings = discovery::load_settings(&config_dir(&app)?);
            let (cd, xd) = discovery::effective_dirs(&settings);
            let path = PathBuf::from(&file_path);
            let source = if inside_dirs(&path, &cd) {
                Source::Claude
            } else if inside_dirs(&path, &xd) {
                Source::Codex
            } else {
                return Err("path outside log directories".into());
            };
            let app2 = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let state = app2.state::<AppState>();
                parse_cached(&state.parse_cache, source, &path)
            })
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("cannot parse {file_path}"))?
        }
    };
    let prices = state.pricing.read().map_err(|e| e.to_string())?;
    Ok(aggregate::detail(&session, &prices))
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
