use crate::model::{ParsedSession, ParsedTurn, Source, TokenUsage};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

fn tok(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// `<root>/brain/<id>/.system_generated/logs/transcript.jsonl` -> `<id>` dir.
pub fn session_dir(path: &Path) -> Option<&Path> {
    let logs = path.parent()?;
    let sys = logs.parent()?;
    (logs.file_name()? == "logs" && sys.file_name()? == ".system_generated").then_some(())?;
    sys.parent()
}

/// Real data JSON-quotes some strings (`"\"/x\""`); strip one quoting layer.
fn unquote(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        return serde_json::from_str::<String>(s)
            .unwrap_or_else(|_| s.trim_matches('"').to_owned());
    }
    s.to_owned()
}

/// `<root>/conversations/<id>.meta` for a transcript path (root = parent of `brain`).
pub fn meta_path(path: &Path) -> Option<PathBuf> {
    let dir = session_dir(path)?;
    let root = dir.parent()?.parent()?;
    Some(
        root.join("conversations")
            .join(format!("{}.meta", dir.file_name()?.to_str()?)),
    )
}

pub fn parse_file(path: &Path) -> anyhow::Result<ParsedSession> {
    let reader = BufReader::new(File::open(path)?);

    let mut cwd: Option<String> = None;
    let mut turns: Vec<ParsedTurn> = Vec::new();
    let mut by_key: HashMap<i64, usize> = HashMap::new();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("type").and_then(Value::as_str) != Some("PLANNER_RESPONSE") {
            continue;
        }

        if cwd.is_none() {
            cwd = v
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|c| c.get("args")?.get("Cwd")?.as_str())
                .map(unquote)
                .find(|c| !c.is_empty());
        }

        let step = v.get("step_index").and_then(Value::as_i64);
        let turn = ParsedTurn {
            timestamp: v
                .get("created_at")
                .and_then(Value::as_str)
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc)),
            model: String::new(),
            usage: TokenUsage {
                input: tok(&v, "input_tokens"),
                output: tok(&v, "output_tokens"),
                cache_creation: 0,
                cache_read: tok(&v, "cache_read_tokens"),
                reasoning: 0,
            },
            dedup_key: step.map(|s| format!("step-{s}")),
        };

        match step {
            Some(k) => match by_key.get(&k) {
                // Last line wins (RUNNING then DONE), first position kept.
                // Skip lines without token keys so they don't zero earlier usage.
                Some(&i) => {
                    if ["input_tokens", "cache_read_tokens", "output_tokens"]
                        .iter()
                        .any(|k| v.get(*k).is_some())
                    {
                        turns[i].usage = turn.usage;
                        turns[i].timestamp = turn.timestamp;
                    }
                }
                None => {
                    by_key.insert(k, turns.len());
                    turns.push(turn);
                }
            },
            None => turns.push(turn),
        }
    }

    let id = session_dir(path)
        .and_then(Path::file_name)
        .or_else(|| path.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let meta = read_meta(path);
    let meta_str = |k: &str| meta.as_ref()?.get(k)?.as_str().map(String::from);
    let model = meta_str("model_id").unwrap_or_else(|| "unknown".to_string());
    for t in &mut turns {
        t.model = model.clone();
    }

    Ok(ParsedSession {
        id,
        source: Source::Antigravity,
        project: meta_str("cwd").map(|c| unquote(&c)).or(cwd),
        file_path: path.to_string_lossy().into_owned(),
        turns,
        agent_id: None,
        parent_session_id: None,
        is_subagent: false,
        agent_label: None,
        agent_type: None,
        log_deleted: false,
    })
}

/// `<root>/conversations/<id>.meta`; None if missing or invalid.
fn read_meta(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(meta_path(path)?).ok()?;
    serde_json::from_str(&text).ok().filter(Value::is_object)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACP: &str = "acp/brain/aaaaaaaa-0000-4000-8000-000000000001";

    fn fixture(dir: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/antigravity")
            .join(dir)
            .join(".system_generated/logs/transcript.jsonl")
    }

    #[test]
    fn acp_session_dedup_order_meta() {
        let s = parse_file(&fixture(ACP)).unwrap();
        assert_eq!(s.id, "aaaaaaaa-0000-4000-8000-000000000001");
        assert_eq!(s.source, Source::Antigravity);
        assert!(!s.is_subagent && !s.log_deleted);
        // meta cwd beats tool-call Cwd
        assert_eq!(s.project.as_deref(), Some("/home/dev/acp-proj"));
        // step 2 first (file order), step 1 deduped to the DONE line; bad/blank/non-planner skipped
        assert_eq!(s.turns.len(), 2);
        let (a, b) = (&s.turns[0], &s.turns[1]);
        assert_eq!(a.dedup_key.as_deref(), Some("step-2"));
        assert_eq!(a.model, "gemini-3.8-flash-medium");
        let u = |t: &ParsedTurn| {
            (
                t.usage.input,
                t.usage.cache_read,
                t.usage.output,
                t.usage.cache_creation,
                t.usage.reasoning,
            )
        };
        assert_eq!(u(a), (100, 4000, 50, 0, 0));
        assert!(a.timestamp.is_some());
        assert_eq!(b.dedup_key.as_deref(), Some("step-1"));
        assert_eq!(u(b), (10, 200, 30, 0, 0));
    }

    #[test]
    fn plain_without_meta_uses_unknown_and_tool_cwd() {
        let s = parse_file(&fixture("plain/brain/bbbbbbbb-0000-4000-8000-000000000002")).unwrap();
        assert_eq!(s.id, "bbbbbbbb-0000-4000-8000-000000000002");
        assert_eq!(s.project.as_deref(), Some("/home/dev/plain-proj"));
        assert_eq!(s.turns.len(), 1);
        assert_eq!(s.turns[0].model, "unknown");
    }

    #[test]
    fn quoted_cwd_is_unquoted_and_tokenless_repeat_keeps_usage() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("x/.system_generated/logs/transcript.jsonl");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            concat!(
                r#"{"step_index":1,"type":"PLANNER_RESPONSE","created_at":"2026-10-21T06:00:00Z","input_tokens":5,"output_tokens":7,"tool_calls":[{"args":{"Cwd":"\"/Users/x/proj\""}}]}"#,
                "\n",
                r#"{"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE"}"#,
                "\n"
            ),
        )
        .unwrap();
        let s = parse_file(&p).unwrap();
        assert_eq!(s.project.as_deref(), Some("/Users/x/proj"));
        assert_eq!(s.turns.len(), 1);
        assert_eq!((s.turns[0].usage.input, s.turns[0].usage.output), (5, 7));
        assert!(s.turns[0].timestamp.is_some());
        assert_eq!(unquote("\"bad"), "\"bad");
    }

    #[test]
    fn missing_meta_acp_unknown_no_project() {
        let s = parse_file(&fixture("acp/brain/cccccccc-0000-4000-8000-000000000003")).unwrap();
        assert_eq!(s.turns[0].model, "unknown");
        assert!(s.project.is_none());
    }

    #[test]
    fn invalid_meta_is_none() {
        let s = parse_file(&fixture("acp/brain/dddddddd-0000-4000-8000-000000000004")).unwrap();
        assert_eq!(s.turns[0].model, "unknown");
        assert!(s.project.is_none());
    }

    #[test]
    fn meta_path_derivation() {
        let p = fixture(ACP);
        assert!(meta_path(&p)
            .unwrap()
            .ends_with("acp/conversations/aaaaaaaa-0000-4000-8000-000000000001.meta"));
    }

    #[test]
    fn id_falls_back_to_file_stem() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("t.jsonl");
        std::fs::write(&p, "{}\n").unwrap();
        let s = parse_file(&p).unwrap();
        assert_eq!(s.id, "t");
        assert!(s.turns.is_empty());
    }

    #[test]
    fn missing_file_errs() {
        assert!(parse_file(&fixture("nope")).is_err());
    }
}
