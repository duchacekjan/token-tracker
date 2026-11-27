use crate::model::{ParsedSession, ParsedTurn, Source, TokenUsage};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

fn apply_record(record: Value, metadata: &mut Value, messages: &mut Vec<Value>) {
    if let Some(id) = record.get("$rewindTo").and_then(Value::as_str) {
        // Unknown target clears everything, like the Gemini CLI reader.
        match messages.iter().position(|m| m["id"].as_str() == Some(id)) {
            Some(index) => messages.truncate(index),
            None => messages.clear(),
        }
    } else if let Some(patch) = record.get("$patch") {
        if let Some(ids) = patch.get("removeIds").and_then(Value::as_array) {
            messages.retain(|m| !ids.iter().any(|id| id == &m["id"]));
        }
    } else if let Some(set) = record.get("$set").and_then(Value::as_object) {
        if let Some(items) = set.get("messages").and_then(Value::as_array) {
            *messages = items.clone();
        }
        metadata.as_object_mut().unwrap().extend(set.clone());
    } else if record.get("id").and_then(Value::as_str).is_some() {
        if let Some(index) = messages.iter().position(|m| m["id"] == record["id"]) {
            messages[index] = record;
        } else {
            messages.push(record);
        }
    } else if let Some(obj) = record.as_object() {
        if let Some(items) = obj.get("messages").and_then(Value::as_array) {
            *messages = items.clone();
        }
        metadata.as_object_mut().unwrap().extend(obj.clone());
    }
}

pub(crate) fn project_root_path(path: &Path) -> Option<PathBuf> {
    Some(path.parent()?.parent()?.join(".project_root"))
}

/// Real project path from `<slug>/.project_root` (file lives in `<slug>/chats/`).
fn project_root(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(project_root_path(path)?).ok()?;
    Some(text.trim().to_owned()).filter(|s| !s.is_empty())
}

pub fn parse_file(path: &Path) -> anyhow::Result<ParsedSession> {
    let mut metadata = Value::Object(Default::default());
    let mut messages: Vec<Value> = Vec::new();
    let legacy = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if legacy {
        // Legacy format: one JSON object with a `messages` array.
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        if let Some(items) = doc.get("messages").and_then(Value::as_array) {
            messages = items.clone();
        }
        metadata = doc;
    } else {
        for line in BufReader::new(std::fs::File::open(path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let Ok(record) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            apply_record(record, &mut metadata, &mut messages);
        }
    }
    let id = metadata["sessionId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    let turns = messages
        .iter()
        .filter(|m| m["type"] == "gemini")
        .filter_map(|m| {
            let tokens = m.get("tokens")?.as_object()?;
            let num = |key: &str| tokens.get(key).and_then(Value::as_u64).unwrap_or(0);
            let input = num("input");
            let cached = num("cached");
            let tool = num("tool");
            let thoughts = num("thoughts");
            Some(ParsedTurn {
                timestamp: m["timestamp"]
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&Utc)),
                model: m["model"].as_str().unwrap_or("unknown").to_owned(),
                usage: TokenUsage {
                    input: input.saturating_sub(cached).saturating_add(tool),
                    output: num("output").saturating_add(thoughts),
                    cache_creation: 0,
                    cache_read: cached,
                    reasoning: thoughts,
                },
                dedup_key: None,
            })
        })
        .collect();
    Ok(ParsedSession {
        id,
        source: Source::Gemini,
        project: project_root(path).or_else(|| metadata["projectHash"].as_str().map(str::to_owned)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/gemini")
            .join(name)
    }

    fn u(t: &ParsedTurn) -> (u64, u64, u64, u64) {
        (
            t.usage.input,
            t.usage.output,
            t.usage.cache_read,
            t.usage.reasoning,
        )
    }

    #[test]
    fn normal_session() {
        let s = parse_file(&fixture("proj/chats/session-normal.jsonl")).unwrap();
        assert_eq!(s.id, "sess-gem-1");
        assert_eq!(s.source, Source::Gemini);
        assert_eq!(s.project.as_deref(), Some("/home/u/proj"));
        assert_eq!(s.turns.len(), 1);
        let t = &s.turns[0];
        assert_eq!(t.model, "gemini-2.5-pro");
        assert_eq!(u(t), (70, 25, 30, 5));
        assert!(t.timestamp.is_some());
    }

    #[test]
    fn upsert_counts_once() {
        // m2 written without tokens, then re-upserted with tokens; m3 removed by $patch.
        let s = parse_file(&fixture("proj/chats/session-normal.jsonl")).unwrap();
        assert_eq!(s.turns.len(), 1);
    }

    #[test]
    fn bad_json_lines_skipped() {
        let s = parse_file(&fixture("proj/chats/session-badline.jsonl")).unwrap();
        assert_eq!(s.id, "sess-bad");
        assert_eq!(s.turns.len(), 2);
    }

    #[test]
    fn rewind_missing_target_clears() {
        let s = parse_file(&fixture("proj/chats/session-rewind-missing.jsonl")).unwrap();
        assert_eq!(s.turns.len(), 1);
        assert_eq!(s.turns[0].usage.input, 4);
    }

    #[test]
    fn tool_tokens_added_to_input() {
        let s = parse_file(&fixture("proj/chats/session-tool.jsonl")).unwrap();
        assert_eq!(u(&s.turns[0]), (78, 25, 30, 5));
    }

    #[test]
    fn empty_session_id_falls_back_to_file_stem() {
        let s = parse_file(&fixture("proj/chats/session-empty-id.jsonl")).unwrap();
        assert_eq!(s.id, "session-empty-id");
    }

    #[test]
    fn legacy_json_session() {
        let s = parse_file(&fixture(
            "proj/chats/session-2026-03-25T05-34-abcd1234.json",
        ))
        .unwrap();
        assert_eq!(s.id, "sess-legacy");
        assert_eq!(s.project.as_deref(), Some("/home/u/proj"));
        assert_eq!(s.turns.len(), 1);
        assert_eq!(u(&s.turns[0]), (40, 12, 10, 2));
    }

    #[test]
    fn project_hash_fallback() {
        let s = parse_file(&fixture("nohash/chats/session-nohash.jsonl")).unwrap();
        assert_eq!(s.project.as_deref(), Some("deadbeefhash"));
    }

    #[test]
    fn missing_file_errs() {
        assert!(parse_file(&fixture("nope.jsonl")).is_err());
    }
}
