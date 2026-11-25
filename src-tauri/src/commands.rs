use crate::aggregate;
use crate::discovery::{self, Settings, SettingsView};
use crate::model::{ParsedSession, SessionDetail, SessionSummary, Source};
use crate::parsers::{claude, codex};
use crate::pricing::{PriceTable, PricingStatus};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::SystemTime;
use tauri::{Manager, State};

/// One cached parse per file path; valid only while mtime and len match.
pub struct CacheEntry {
    mtime: Option<SystemTime>,
    len: u64,
    session: Arc<ParsedSession>,
}
type ParseCache = HashMap<PathBuf, CacheEntry>;

pub struct AppState {
    pub pricing: RwLock<PriceTable>,
    /// Deduped sessions from the last scan, keyed by file path.
    pub sessions: Mutex<HashMap<String, ParsedSession>>,
    /// Raw parse results, one entry per file path.
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

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Return the cached parse for `path` if mtime+len match, else run `parse`
/// and replace the entry.
fn refresh_entry<F>(
    cache: &mut ParseCache,
    path: &Path,
    mtime: Option<SystemTime>,
    len: u64,
    parse: F,
) -> Option<Arc<ParsedSession>>
where
    F: FnOnce() -> Option<ParsedSession>,
{
    if let Some(e) = cache.get(path) {
        if e.mtime == mtime && e.len == len {
            return Some(Arc::clone(&e.session));
        }
    }
    let session = Arc::new(parse()?);
    cache.insert(
        path.to_path_buf(),
        CacheEntry {
            mtime,
            len,
            session: Arc::clone(&session),
        },
    );
    Some(session)
}

/// Drop entries whose path is not in `live`.
fn prune_cache(cache: &mut ParseCache, live: &HashSet<&Path>) {
    cache.retain(|p, _| live.contains(p.as_path()));
}

/// Parse with cache. Returns an owned copy because dedup mutates sessions.
fn parse_cached(cache: &Mutex<ParseCache>, source: Source, path: &Path) -> Option<ParsedSession> {
    let m = std::fs::metadata(path).ok()?;
    let (mtime, len) = (m.modified().ok(), m.len());
    if let Some(e) = lock(cache).get(path) {
        if e.mtime == mtime && e.len == len {
            return Some((*e.session).clone());
        }
    }
    // Parse outside the lock so other scans are not blocked.
    let parsed = match source {
        Source::Claude => claude::parse_file(path),
        Source::Codex => codex::parse_file(path),
    }
    .ok()?;
    let out = parsed.clone();
    refresh_entry(&mut lock(cache), path, mtime, len, || Some(parsed));
    Some(out)
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

/// Full scan (cross-session Claude dedup applies), refreshing the parse cache.
async fn scan_all(app: &tauri::AppHandle) -> Result<Vec<ParsedSession>, String> {
    let settings = discovery::load_settings(&config_dir(app)?);
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (cd, xd) = discovery::effective_dirs(&settings);
        let cf = discovery::find_jsonl(&cd);
        let xf = discovery::find_jsonl(&xd);
        let sessions =
            aggregate::scan_with(&cf, &xf, |src, p| parse_cached(&state.parse_cache, src, p));
        let live: HashSet<&Path> = cf.iter().chain(xf.iter()).map(PathBuf::as_path).collect();
        prune_cache(&mut lock(&state.parse_cache), &live);
        sessions
    })
    .await
    .map_err(|e| e.to_string())
}

fn store_sessions(state: &AppState, sessions: &[ParsedSession]) {
    let map: HashMap<String, ParsedSession> = sessions
        .iter()
        .map(|s| (s.file_path.clone(), s.clone()))
        .collect();
    *lock(&state.sessions) = map;
}

#[tauri::command]
pub async fn list_sessions(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SessionSummary>, String> {
    let sessions = scan_all(&app).await?;

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
    store_sessions(&state, &sessions);
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

fn is_jsonl(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
}

#[tauri::command]
pub async fn get_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_path: String,
) -> Result<SessionDetail, String> {
    let stored = lock(&state.sessions).get(&file_path).cloned();
    let session = match stored {
        Some(s) => s,
        None => {
            let settings = discovery::load_settings(&config_dir(&app)?);
            let (cd, xd) = discovery::effective_dirs(&settings);
            let path = PathBuf::from(&file_path);
            if !inside_dirs(&path, &cd) && !inside_dirs(&path, &xd) {
                return Err("path outside log directories".into());
            }
            if !is_jsonl(&path) {
                return Err("not a .jsonl file".into());
            }
            let sessions = scan_all(&app).await?;
            store_sessions(&state, &sessions);
            sessions
                .into_iter()
                .find(|s| s.file_path == file_path)
                .ok_or_else(|| "session not found".to_string())?
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(id: &str) -> ParsedSession {
        ParsedSession {
            id: id.into(),
            source: Source::Claude,
            project: None,
            file_path: String::new(),
            turns: vec![],
        }
    }

    #[test]
    fn refresh_reuses_when_unchanged() {
        let mut c = ParseCache::new();
        let p = Path::new("/a.jsonl");
        let first = refresh_entry(&mut c, p, None, 10, || Some(sess("1"))).unwrap();
        let second = refresh_entry(&mut c, p, None, 10, || panic!("must not reparse")).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn refresh_replaces_single_entry_on_change() {
        let mut c = ParseCache::new();
        let p = Path::new("/a.jsonl");
        refresh_entry(&mut c, p, None, 10, || Some(sess("old")));
        let s = refresh_entry(&mut c, p, None, 20, || Some(sess("new"))).unwrap();
        assert_eq!(s.id, "new");
        assert_eq!(c.len(), 1);
        assert_eq!(c[p].len, 20);
    }

    #[test]
    fn refresh_failed_parse_keeps_nothing_new() {
        let mut c = ParseCache::new();
        assert!(refresh_entry(&mut c, Path::new("/a"), None, 1, || None).is_none());
        assert!(c.is_empty());
    }

    #[test]
    fn prune_drops_missing_paths() {
        let mut c = ParseCache::new();
        refresh_entry(&mut c, Path::new("/a"), None, 1, || Some(sess("a")));
        refresh_entry(&mut c, Path::new("/b"), None, 1, || Some(sess("b")));
        let live: HashSet<&Path> = [Path::new("/a")].into_iter().collect();
        prune_cache(&mut c, &live);
        assert!(c.contains_key(Path::new("/a")));
        assert!(!c.contains_key(Path::new("/b")));
    }

    #[test]
    fn jsonl_extension_check() {
        assert!(is_jsonl(Path::new("/x/a.jsonl")));
        assert!(is_jsonl(Path::new("/x/a.JSONL")));
        assert!(!is_jsonl(Path::new("/x/a.json")));
        assert!(!is_jsonl(Path::new("/x/jsonl")));
    }

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
}
