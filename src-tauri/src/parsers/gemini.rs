use crate::model::{ParsedSession, ParsedTurn, Source, TokenUsage};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn parse_file(path: &Path) -> anyhow::Result<ParsedSession> {
    let mut metadata = Value::Object(Default::default());
    let mut messages: Vec<Value> = Vec::new();
    for line in BufReader::new(std::fs::File::open(path)?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: Value = serde_json::from_str(&line)?;
        if let Some(id) = record.get("$rewindTo").and_then(Value::as_str) {
            if let Some(index) = messages.iter().position(|m| m["id"].as_str() == Some(id)) {
                messages.truncate(index);
            }
        } else if let Some(patch) = record.get("$patch") {
            if let Some(ids) = patch.get("removeIds").and_then(Value::as_array) {
                messages.retain(|m| !ids.iter().any(|id| id == &m["id"]));
            }
        } else if let Some(set) = record.get("$set").and_then(Value::as_object) {
            if let Some(items) = set.get("messages").and_then(Value::as_array) {
                messages = items.clone();
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
                messages = items.clone();
            }
            metadata.as_object_mut().unwrap().extend(obj.clone());
        }
    }
    let id = metadata["sessionId"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing Gemini session ID"))?
        .to_owned();
    let turns = messages
        .iter()
        .filter(|m| m["type"] == "gemini")
        .filter_map(|m| {
            let tokens = m.get("tokens")?.as_object()?;
            let num = |key: &str| tokens.get(key).and_then(Value::as_u64).unwrap_or(0);
            let input = num("input");
            let cached = num("cached");
            let thoughts = num("thoughts");
            Some(ParsedTurn {
                timestamp: m["timestamp"]
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&Utc)),
                model: m["model"].as_str().unwrap_or("unknown").to_owned(),
                usage: TokenUsage {
                    input: input.saturating_sub(cached),
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
        project: metadata["projectHash"].as_str().map(str::to_owned),
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
