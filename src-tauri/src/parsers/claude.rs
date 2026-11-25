use crate::model::{ParsedSession, ParsedTurn, Source, TokenUsage};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

fn tok(usage: &Value, key: &str) -> u64 {
    usage.get(key).and_then(Value::as_u64).unwrap_or(0)
}

pub fn parse_file(path: &Path) -> anyhow::Result<ParsedSession> {
    let reader = BufReader::new(File::open(path)?);

    let mut session_id: Option<String> = None;
    let mut project: Option<String> = None;
    let mut agent_id: Option<String> = None;
    let mut first_sidechain: Option<bool> = None;
    let mut turns: Vec<ParsedTurn> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if session_id.is_none() {
            session_id = v.get("sessionId").and_then(Value::as_str).map(String::from);
        }
        if project.is_none() {
            project = v.get("cwd").and_then(Value::as_str).map(String::from);
        }

        if agent_id.is_none() {
            agent_id = v.get("agentId").and_then(Value::as_str).map(String::from);
        }
        let sidechain = v.get("isSidechain").and_then(Value::as_bool);
        if first_sidechain.is_none() {
            first_sidechain = sidechain;
        }

        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        let Some(usage) = msg.get("usage").filter(|u| u.is_object()) else {
            continue;
        };
        let Some(model) = msg.get("model").and_then(Value::as_str) else {
            continue;
        };
        if model == "<synthetic>" {
            continue;
        }

        let msg_id = msg.get("id").and_then(Value::as_str);
        let req_id = v.get("requestId").and_then(Value::as_str);
        let dedup_key = match (msg_id, req_id) {
            (Some(m), Some(r)) => Some(format!("{m}:{r}")),
            (Some(m), None) => Some(m.to_string()),
            _ => None,
        };

        let turn = ParsedTurn {
            timestamp: v
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc)),
            model: model.to_string(),
            usage: TokenUsage {
                input: tok(usage, "input_tokens"),
                output: tok(usage, "output_tokens"),
                cache_creation: tok(usage, "cache_creation_input_tokens"),
                cache_read: tok(usage, "cache_read_input_tokens"),
                reasoning: 0,
            },
            dedup_key: dedup_key.clone(),
        };

        match dedup_key {
            Some(k) => match by_key.get(&k) {
                // Last occurrence's usage wins, first occurrence's position kept.
                Some(&i) => turns[i].usage = turn.usage,
                None => {
                    by_key.insert(k, turns.len());
                    turns.push(turn);
                }
            },
            None => turns.push(turn),
        }
    }

    let id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });

    let in_subagents_dir = path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "subagents");
    let is_subagent = in_subagents_dir || (agent_id.is_some() && first_sidechain == Some(true));

    let meta = read_meta(path);
    let meta_str = |k: &str| meta.as_ref()?.get(k)?.as_str().map(String::from);

    Ok(ParsedSession {
        id,
        source: Source::Claude,
        project,
        file_path: path.to_string_lossy().into_owned(),
        turns,
        agent_id,
        is_subagent,
        agent_label: meta_str("description"),
        agent_type: meta_str("agentType"),
        log_deleted: false,
    })
}

/// Sibling `<stem>.meta.json`; None if missing or invalid.
fn read_meta(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path.with_extension("meta.json")).ok()?;
    serde_json::from_str(&text).ok().filter(Value::is_object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude")
            .join(name)
    }

    #[test]
    fn normal_session() {
        let s = parse_file(&fixture("normal.jsonl")).unwrap();
        assert_eq!(s.id, "sess-1111");
        assert_eq!(s.source, Source::Claude);
        assert_eq!(s.project.as_deref(), Some("/home/dev/proj"));
        assert_eq!(s.turns.len(), 2);
        let t = &s.turns[0];
        assert_eq!(t.model, "claude-sonnet-4-5");
        assert_eq!(t.dedup_key.as_deref(), Some("msg_1:req_1"));
        assert_eq!(
            (
                t.usage.input,
                t.usage.output,
                t.usage.cache_creation,
                t.usage.cache_read,
                t.usage.reasoning
            ),
            (10, 20, 300, 4000, 0)
        );
        assert!(t.timestamp.is_some());
        assert_eq!(s.turns[1].model, "claude-opus-4-1");
    }

    #[test]
    fn duplicate_streaming_lines_keep_last_usage_first_position() {
        let s = parse_file(&fixture("duplicates.jsonl")).unwrap();
        assert_eq!(s.turns.len(), 2);
        assert_eq!(s.turns[0].dedup_key.as_deref(), Some("msg_1:req_1"));
        assert_eq!(s.turns[0].usage.output, 88);
        assert_eq!(s.turns[1].dedup_key.as_deref(), Some("msg_2:req_2"));
        assert_eq!(s.turns[1].usage.output, 9);
    }

    #[test]
    fn synthetic_and_usage_less_skipped() {
        let s = parse_file(&fixture("synthetic.jsonl")).unwrap();
        assert_eq!(s.turns.len(), 1);
        assert_eq!(s.turns[0].model, "claude-sonnet-4-5");
    }

    #[test]
    fn bad_json_lines_skipped() {
        let s = parse_file(&fixture("badline.jsonl")).unwrap();
        assert_eq!(s.id, "sess-4444");
        assert_eq!(s.turns.len(), 2);
    }

    #[test]
    fn missing_cache_fields_default_zero() {
        let s = parse_file(&fixture("synthetic.jsonl")).unwrap();
        let u = &s.turns[0].usage;
        assert_eq!(
            (u.input, u.output, u.cache_creation, u.cache_read),
            (3, 4, 0, 0)
        );
    }

    #[test]
    fn session_id_falls_back_to_file_stem() {
        let s = parse_file(&fixture("no-session-id.jsonl")).unwrap();
        assert_eq!(s.id, "no-session-id");
        assert_eq!(s.turns[0].dedup_key.as_deref(), Some("msg_9"));
    }

    const SUB_DIR: &str = "11111111-2222-4333-8444-555555555555/subagents";

    #[test]
    fn main_transcript_not_subagent() {
        let s = parse_file(&fixture("normal.jsonl")).unwrap();
        assert!(!s.is_subagent);
        assert!(s.agent_id.is_none() && s.agent_label.is_none() && s.agent_type.is_none());
        assert!(!s.log_deleted);
    }

    #[test]
    fn subagent_with_meta() {
        let s = parse_file(&fixture(&format!("{SUB_DIR}/agent-abc.jsonl"))).unwrap();
        assert_eq!(s.id, "11111111-2222-4333-8444-555555555555");
        assert!(s.is_subagent);
        assert_eq!(s.agent_id.as_deref(), Some("abc"));
        assert_eq!(s.agent_label.as_deref(), Some("Scaffold Tauri Svelte app"));
        assert_eq!(s.agent_type.as_deref(), Some("itixo:itixo-builder"));
        assert_eq!(s.turns.len(), 1);
    }

    #[test]
    fn subagent_missing_meta() {
        let s = parse_file(&fixture(&format!("{SUB_DIR}/agent-nometa.jsonl"))).unwrap();
        assert!(s.is_subagent);
        assert_eq!(s.agent_id.as_deref(), Some("nometa"));
        assert!(s.agent_label.is_none() && s.agent_type.is_none());
    }

    #[test]
    fn subagent_invalid_meta() {
        let s = parse_file(&fixture(&format!("{SUB_DIR}/agent-badmeta.jsonl"))).unwrap();
        assert!(s.is_subagent);
        assert!(s.agent_label.is_none() && s.agent_type.is_none());
    }

    #[test]
    fn inline_sidechain_turn_keeps_main_transcript() {
        let s = parse_file(&fixture("inline-sidechain.jsonl")).unwrap();
        assert!(!s.is_subagent);
        assert_eq!(s.turns.len(), 2);
    }

    #[test]
    fn agent_id_with_sidechain_first_line_outside_subagents_dir() {
        let s = parse_file(&fixture("loose-agent.jsonl")).unwrap();
        assert!(s.is_subagent);
        assert_eq!(s.agent_id.as_deref(), Some("loose"));
    }

    #[test]
    fn missing_file_errs() {
        assert!(parse_file(&fixture("nope.jsonl")).is_err());
    }
}
