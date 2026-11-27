use crate::aggregate::{self, SessionGroup};
use crate::db::{Store, SyncStats};
use crate::discovery::{self, Settings, SettingsView};
use crate::model::{SessionDetail, SessionSummary, Source};
use crate::parsers::{claude, codex, gemini};
use crate::pricing::{PriceTable, PricingStatus};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, RwLock};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

/// Group map key. `Source` is not `Hash`, so use its stable name.
type GroupKey = (&'static str, String);

fn group_key(source: Source, id: &str) -> GroupKey {
    let src = match source {
        Source::Claude => "claude",
        Source::Codex => "codex",
        Source::Gemini => "gemini",
    };
    (src, id.to_string())
}

pub struct AppState {
    pub pricing: RwLock<PriceTable>,
    pub store: Mutex<Store>,
    /// Groups from the last list run, for detail lookups.
    pub groups: Mutex<HashMap<GroupKey, SessionGroup>>,
    /// Set at startup when the DB had to be recovered or is in-memory only.
    pub db_warning: Option<String>,
}

impl AppState {
    pub fn new(pricing: PriceTable, store: Store, db_warning: Option<String>) -> Self {
        Self {
            pricing: RwLock::new(pricing),
            store: Mutex::new(store),
            groups: Mutex::new(HashMap::new()),
            db_warning,
        }
    }
}

#[tauri::command]
pub fn get_db_warning(state: State<'_, AppState>) -> Option<String> {
    state.db_warning.clone()
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
    let (c, x, g) = discovery::effective_dirs(&settings);
    let strs = |v: Vec<PathBuf>| v.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let pricing = state.pricing.read().map_err(|e| e.to_string())?.status();
    Ok(SettingsView {
        settings,
        claude_dirs: strs(c),
        codex_dirs: strs(x),
        gemini_dirs: strs(g),
        pricing,
    })
}

fn parse_any(source: Source, path: &Path) -> anyhow::Result<crate::model::ParsedSession> {
    match source {
        Source::Claude => claude::parse_file(path),
        Source::Codex => codex::parse_file(path),
        Source::Gemini => gemini::parse_file(path),
    }
}

/// sync -> load_all -> build. Pure over its inputs (no Tauri state).
fn run_pipeline(
    store: &mut Store,
    claude_files: &[PathBuf],
    codex_files: &[PathBuf],
    gemini_files: &[PathBuf],
    prices: &PriceTable,
) -> anyhow::Result<(SyncStats, Vec<SessionGroup>, Vec<SessionSummary>)> {
    let stats = store.sync(claude_files, codex_files, gemini_files, parse_any)?;
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
        let (cd, xd, gd) = discovery::effective_dirs(&settings);
        let cf = discovery::find_jsonl(&cd);
        let xf = discovery::find_jsonl(&xd);
        let gf: Vec<_> = discovery::find_jsonl(&gd)
            .into_iter()
            .filter(|p| {
                p.parent()
                    .and_then(|p| p.file_name())
                    .is_some_and(|n| n == "chats")
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("session-"))
            })
            .collect();
        let (_, groups, summaries) = {
            let prices = state.pricing.read().map_err(|e| e.to_string())?;
            run_pipeline(&mut lock(&state.store), &cf, &xf, &gf, &prices)
                .map_err(|e| e.to_string())?
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionKey {
    pub source: Source,
    pub id: String,
}

/// Details for `keys` in input order, plus the keys not present in `groups`.
fn details_for(
    groups: &HashMap<GroupKey, SessionGroup>,
    keys: &[SessionKey],
    prices: &PriceTable,
) -> (Vec<SessionDetail>, Vec<SessionKey>) {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for k in keys {
        match groups.get(&group_key(k.source, &k.id)) {
            Some(g) => found.push(aggregate::detail_group(g, prices)),
            None => missing.push(k.clone()),
        }
    }
    (found, missing)
}

#[tauri::command]
pub async fn get_sessions_detail(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    keys: Vec<SessionKey>,
) -> Result<Vec<SessionDetail>, String> {
    let lookup = |state: &AppState, keys: &[SessionKey]| {
        let groups = lock(&state.groups);
        let prices = state.pricing.read().map_err(|e| e.to_string())?;
        Ok::<_, String>(details_for(&groups, keys, &prices))
    };
    let (found, missing) = lookup(&state, &keys)?;
    if missing.is_empty() {
        return Ok(found);
    }
    refresh(&app).await?;
    let (retry, _still_missing) = lookup(&state, &missing)?;
    // Merge back in input order.
    let mut by_key: HashMap<GroupKey, SessionDetail> = found
        .into_iter()
        .chain(retry)
        .map(|d| (group_key(d.summary.source, &d.summary.id), d))
        .collect();
    Ok(keys
        .iter()
        .filter_map(|k| by_key.remove(&group_key(k.source, &k.id)))
        .collect())
}

/// Append ".csv" unless the extension is already csv (case-insensitive).
fn ensure_csv_ext(path: PathBuf) -> PathBuf {
    let is_csv = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("csv"));
    if is_csv {
        return path;
    }
    let mut os = path.into_os_string();
    os.push(".csv");
    PathBuf::from(os)
}

/// Write via temp file in the same dir, then rename over `path`. Parent must exist.
fn write_text_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mut tmp = tempfile::Builder::new()
        .prefix(".token-tracker-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(contents.as_bytes())?;
    tmp.flush()?;
    // tempfile creates 0600; keep the existing file's mode, else use a normal 0644.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o7777)
            .unwrap_or(0o644);
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[tauri::command]
pub async fn save_csv(
    app: tauri::AppHandle,
    default_name: String,
    contents: String,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(chosen) = app
            .dialog()
            .file()
            .add_filter("CSV", &["csv"])
            .set_file_name(default_name)
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = ensure_csv_ext(chosen.into_path().map_err(|e| e.to_string())?);
        write_text_atomic(&path, &contents).map_err(|e| e.to_string())?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
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

        let (s1, groups, sums) = run_pipeline(&mut store, &cf, &xf, &[], &prices).unwrap();
        assert!(s1.parsed > 0);
        assert_eq!(groups.len(), sums.len());
        assert!(groups.iter().any(|g| g.source == Source::Codex));
        assert!(sums
            .iter()
            .any(|s| s.source == Source::Claude && s.subagent_count > 0));

        let (s2, groups2, sums2) = run_pipeline(&mut store, &cf, &xf, &[], &prices).unwrap();
        assert_eq!(s2.parsed, 0);
        assert_eq!(s2.unchanged, s1.parsed);
        assert_eq!(groups2.len(), groups.len());
        assert_eq!(sums2.len(), sums.len());
    }

    fn fixture_groups() -> (HashMap<GroupKey, SessionGroup>, PriceTable) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cf = discovery::find_jsonl(&[root.join("claude")]);
        let xf = discovery::find_jsonl(&[root.join("codex")]);
        let prices = PriceTable::load(Path::new("/nonexistent-cache-dir"));
        let mut store = Store::open_in_memory().unwrap();
        let (_, groups, _) = run_pipeline(&mut store, &cf, &xf, &[], &prices).unwrap();
        let map = groups
            .into_iter()
            .map(|g| (group_key(g.source, &g.id), g))
            .collect();
        (map, prices)
    }

    #[test]
    fn details_for_order_and_missing() {
        let (groups, prices) = fixture_groups();
        let mut ks: Vec<SessionKey> = groups
            .values()
            .map(|g| SessionKey {
                source: g.source,
                id: g.id.clone(),
            })
            .collect();
        ks.sort_by(|a, b| a.id.cmp(&b.id));
        assert!(ks.len() >= 2);
        ks.reverse();
        let ghost = SessionKey {
            source: Source::Codex,
            id: "no-such-session".into(),
        };
        let mut input = ks.clone();
        input.insert(1, ghost.clone());
        let (found, missing) = details_for(&groups, &input, &prices);
        assert_eq!(missing, vec![ghost]);
        let ids: Vec<&str> = found.iter().map(|d| d.summary.id.as_str()).collect();
        let want: Vec<&str> = ks.iter().map(|k| k.id.as_str()).collect();
        assert_eq!(ids, want);
    }

    #[test]
    fn session_key_deserializes_camel() {
        let k: SessionKey = serde_json::from_str(r#"{"source":"claude","id":"x"}"#).unwrap();
        assert_eq!(k.source, Source::Claude);
    }

    #[test]
    fn csv_ext_cases() {
        let f = |s: &str| ensure_csv_ext(PathBuf::from(s));
        assert_eq!(f("/a/b.csv"), PathBuf::from("/a/b.csv"));
        assert_eq!(f("/a/b.CSV"), PathBuf::from("/a/b.CSV"));
        assert_eq!(f("/a/b"), PathBuf::from("/a/b.csv"));
        assert_eq!(f("/a/b.txt"), PathBuf::from("/a/b.txt.csv"));
        assert_eq!(f("/a/.csv"), PathBuf::from("/a/.csv.csv"));
    }

    #[test]
    fn atomic_write_exact_bytes_and_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("o.csv");
        let text = "\u{feff}a;é\r\n1,5;日本\r\n";
        write_text_atomic(&p, text).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), text.as_bytes());
        write_text_atomic(&p, "new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    // Explicit set_permissions makes the result independent of the process umask.
    #[cfg(unix)]
    #[test]
    fn atomic_write_new_file_mode_0644_and_keeps_existing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("o.csv");
        write_text_atomic(&p, "a").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&p), 0o644);
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_text_atomic(&p, "b").unwrap();
        assert_eq!(mode(&p), 0o600);
    }

    #[test]
    fn atomic_write_missing_parent_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope").join("o.csv");
        assert!(write_text_atomic(&p, "x").is_err());
    }
}
