use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub extra_claude_dirs: Vec<String>,
    pub extra_codex_dirs: Vec<String>,
    #[serde(default)]
    pub extra_gemini_dirs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub claude_dirs: Vec<String>,
    pub codex_dirs: Vec<String>,
    pub gemini_dirs: Vec<String>,
    pub pricing: crate::pricing::PricingStatus,
}

/// Keep only existing dirs, dedupe, preserve order.
fn existing_dedup(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for c in candidates {
        if c.is_dir() && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

fn claude_dirs_from(home: Option<PathBuf>, env: Option<String>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    match env.filter(|v| !v.trim().is_empty()) {
        Some(v) => {
            for entry in v.split(',').map(str::trim).filter(|e| !e.is_empty()) {
                candidates.push(PathBuf::from(entry).join("projects"));
            }
        }
        None => {
            if let Some(h) = home {
                candidates.push(h.join(".config").join("claude").join("projects"));
                candidates.push(h.join(".claude").join("projects"));
            }
        }
    }
    existing_dedup(candidates)
}

fn codex_dirs_from(home: Option<PathBuf>, env: Option<String>) -> Vec<PathBuf> {
    let base = match env.filter(|v| !v.trim().is_empty()) {
        Some(v) => PathBuf::from(v.trim()),
        None => match home {
            Some(h) => h.join(".codex"),
            None => return vec![],
        },
    };
    existing_dedup(vec![base.join("sessions"), base.join("archived_sessions")])
}

pub fn default_claude_dirs() -> Vec<PathBuf> {
    claude_dirs_from(dirs::home_dir(), std::env::var("CLAUDE_CONFIG_DIR").ok())
}

pub fn default_codex_dirs() -> Vec<PathBuf> {
    codex_dirs_from(dirs::home_dir(), std::env::var("CODEX_HOME").ok())
}

/// Defaults plus existing user-configured extras, deduped.
pub fn effective_dirs(settings: &Settings) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
    fn merge(mut base: Vec<PathBuf>, extras: &[String]) -> Vec<PathBuf> {
        base.extend(extras.iter().map(PathBuf::from));
        existing_dedup(base)
    }
    (
        merge(default_claude_dirs(), &settings.extra_claude_dirs),
        merge(default_codex_dirs(), &settings.extra_codex_dirs),
        merge(
            existing_dedup(
                dirs::home_dir()
                    .map(|h| vec![h.join(".gemini/tmp")])
                    .unwrap_or_default(),
            ),
            &settings.extra_gemini_dirs,
        ),
    )
}

pub fn find_jsonl(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        for entry in walkdir::WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let is_jsonl = entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"));
            if is_jsonl {
                files.push(entry.into_path());
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

pub fn load_settings(config_dir: &Path) -> Settings {
    std::fs::read_to_string(config_dir.join("settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(config_dir: &Path, s: &Settings) -> anyhow::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let json = serde_json::to_string_pretty(s)?;
    let tmp = config_dir.join("settings.json.tmp");
    std::fs::write(&tmp, json)?;
    if let Err(e) = std::fs::rename(&tmp, config_dir.join("settings.json")) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn mk(p: &Path) -> PathBuf {
        fs::create_dir_all(p).unwrap();
        p.to_path_buf()
    }

    #[test]
    fn claude_env_single() {
        let t = tempdir().unwrap();
        let a = mk(&t.path().join("a/projects"));
        let r = claude_dirs_from(None, Some(t.path().join("a").to_string_lossy().into()));
        assert_eq!(r, vec![a]);
    }

    #[test]
    fn claude_env_comma_separated() {
        let t = tempdir().unwrap();
        let a = mk(&t.path().join("a/projects"));
        let b = mk(&t.path().join("b/projects"));
        let env = format!(
            "{}, {},{}",
            t.path().join("a").display(),
            t.path().join("b").display(),
            t.path().join("a").display()
        );
        assert_eq!(claude_dirs_from(None, Some(env)), vec![a, b]);
    }

    #[test]
    fn claude_empty_env_falls_back_to_home() {
        let t = tempdir().unwrap();
        let a = mk(&t.path().join(".config/claude/projects"));
        let r = claude_dirs_from(Some(t.path().to_path_buf()), Some("".into()));
        assert_eq!(r, vec![a]);
    }

    #[test]
    fn claude_home_both_locations() {
        let t = tempdir().unwrap();
        let a = mk(&t.path().join(".config/claude/projects"));
        let b = mk(&t.path().join(".claude/projects"));
        let r = claude_dirs_from(Some(t.path().to_path_buf()), None);
        assert_eq!(r, vec![a, b]);
    }

    #[test]
    fn nonexistent_filtered() {
        let t = tempdir().unwrap();
        assert!(claude_dirs_from(Some(t.path().to_path_buf()), None).is_empty());
        let env = t.path().join("nope").to_string_lossy().into_owned();
        assert!(claude_dirs_from(None, Some(env)).is_empty());
        assert!(codex_dirs_from(Some(t.path().to_path_buf()), None).is_empty());
        assert!(codex_dirs_from(None, None).is_empty());
    }

    #[test]
    fn codex_home_with_archived() {
        let t = tempdir().unwrap();
        let s = mk(&t.path().join(".codex/sessions"));
        let a = mk(&t.path().join(".codex/archived_sessions"));
        let r = codex_dirs_from(Some(t.path().to_path_buf()), None);
        assert_eq!(r, vec![s, a]);
    }

    #[test]
    fn codex_env_override() {
        let t = tempdir().unwrap();
        let a = mk(&t.path().join("x/archived_sessions"));
        let env = t.path().join("x").to_string_lossy().into_owned();
        let r = codex_dirs_from(Some(PathBuf::from("/nonexistent")), Some(env));
        assert_eq!(r, vec![a]);
    }

    #[test]
    fn find_jsonl_recursive_filtered_sorted() {
        let t = tempdir().unwrap();
        let root = mk(&t.path().join("r"));
        mk(&root.join("sub/deep"));
        fs::write(root.join("b.jsonl"), "").unwrap();
        fs::write(root.join("sub/deep/a.JSONL"), "").unwrap();
        fs::write(root.join("sub/c.json"), "").unwrap();
        fs::write(root.join("d.txt"), "").unwrap();
        fs::create_dir(root.join("dir.jsonl")).unwrap();
        let r = find_jsonl(&[root.clone(), root.clone(), t.path().join("missing")]);
        assert_eq!(r, vec![root.join("b.jsonl"), root.join("sub/deep/a.JSONL")]);
    }

    #[test]
    fn settings_roundtrip() {
        let t = tempdir().unwrap();
        let dir = t.path().join("cfg/nested");
        let s = Settings {
            extra_claude_dirs: vec!["/a".into()],
            extra_gemini_dirs: vec![],
            extra_codex_dirs: vec!["/b".into(), "/c".into()],
        };
        save_settings(&dir, &s).unwrap();
        let l = load_settings(&dir);
        assert_eq!(l.extra_claude_dirs, s.extra_claude_dirs);
        assert_eq!(l.extra_codex_dirs, s.extra_codex_dirs);
        assert!(!dir.join("settings.json.tmp").exists());
    }

    #[test]
    fn settings_missing_or_invalid_is_default() {
        let t = tempdir().unwrap();
        let l = load_settings(t.path());
        assert!(l.extra_claude_dirs.is_empty() && l.extra_codex_dirs.is_empty());
        fs::write(t.path().join("settings.json"), "{not json").unwrap();
        let l = load_settings(t.path());
        assert!(l.extra_claude_dirs.is_empty() && l.extra_codex_dirs.is_empty());
    }

    #[test]
    fn effective_dirs_merges_and_dedups() {
        let t = tempdir().unwrap();
        let x = mk(&t.path().join("x"));
        let y = mk(&t.path().join("y"));
        let s = Settings {
            extra_claude_dirs: vec![
                x.to_string_lossy().into(),
                x.to_string_lossy().into(),
                t.path().join("missing").to_string_lossy().into(),
            ],
            extra_gemini_dirs: vec![],
            extra_codex_dirs: vec![y.to_string_lossy().into()],
        };
        let (c, d, _) = effective_dirs(&s);
        assert_eq!(c.iter().filter(|p| **p == x).count(), 1);
        assert!(!c.iter().any(|p| p.ends_with("missing")));
        assert!(d.contains(&y));
        // defaults come first
        let defaults = default_claude_dirs();
        assert_eq!(&c[..defaults.len()], &defaults[..]);
    }
}
