use crate::model::{ParsedSession, ParsedTurn, Source, TokenUsage};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Raw OpenAI counters: input (includes cached), cached, output (includes reasoning), reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Raw {
    input: u64,
    cached: u64,
    output: u64,
    reasoning: u64,
    total: u64,
}

impl Raw {
    fn from_value(v: &Value) -> Option<Raw> {
        if !v.is_object() {
            return None;
        }
        let f = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        Some(Raw {
            input: f("input_tokens"),
            cached: f("cached_input_tokens"),
            output: f("output_tokens"),
            reasoning: f("reasoning_output_tokens"),
            total: f("total_tokens"),
        })
    }

    fn delta(&self, prev: &Raw) -> Raw {
        Raw {
            input: self.input.saturating_sub(prev.input),
            cached: self.cached.saturating_sub(prev.cached),
            output: self.output.saturating_sub(prev.output),
            reasoning: self.reasoning.saturating_sub(prev.reasoning),
            total: self.total.saturating_sub(prev.total),
        }
    }

    fn into_usage(self) -> TokenUsage {
        TokenUsage {
            input: self.input.saturating_sub(self.cached),
            output: self.output,
            cache_creation: 0,
            cache_read: self.cached,
            reasoning: self.reasoning,
        }
    }
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn fallback_id(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tail: Vec<char> = stem.chars().rev().take(36).collect();
    if tail.len() == 36 {
        let cand: String = tail.into_iter().rev().collect();
        let looks_uuid = cand.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
        if looks_uuid {
            return cand;
        }
    }
    stem
}

pub fn parse_file(path: &Path) -> anyhow::Result<ParsedSession> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);

    let mut meta_id: Option<String> = None;
    let mut meta_cwd: Option<String> = None;
    let mut meta_model: Option<String> = None;
    let mut parent_session_id = None;
    let mut agent_role = None;
    let mut is_subagent = false;
    let mut ctx_cwd: Option<String> = None;
    let mut cur_model: Option<String> = None;
    let mut prev_total: Option<Raw> = None;
    let mut turns = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(payload) = v.get("payload") else {
            continue;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                let spawn = payload.pointer("/source/subagent/thread_spawn");
                is_subagent |=
                    spawn.is_some() || str_at(payload, "thread_source") == Some("subagent");
                if parent_session_id.is_none() {
                    parent_session_id = spawn
                        .and_then(|v| str_at(v, "parent_thread_id"))
                        .map(str::to_owned);
                }
                if agent_role.is_none() {
                    agent_role = str_at(payload, "agent_role")
                        .or_else(|| spawn.and_then(|v| str_at(v, "agent_role")))
                        .map(str::to_owned);
                }
                if meta_id.is_none() {
                    meta_id = str_at(payload, "id").map(str::to_owned);
                }
                if meta_cwd.is_none() {
                    meta_cwd = str_at(payload, "cwd").map(str::to_owned);
                }
                if meta_model.is_none() {
                    meta_model = str_at(payload, "model").map(str::to_owned);
                }
            }
            Some("turn_context") => {
                if let Some(m) = str_at(payload, "model") {
                    cur_model = Some(m.to_owned());
                }
                if ctx_cwd.is_none() {
                    ctx_cwd = str_at(payload, "cwd").map(str::to_owned);
                }
            }
            Some("event_msg") => {
                if str_at(payload, "type") != Some("token_count") {
                    continue;
                }
                let Some(info) = payload.get("info").filter(|i| i.is_object()) else {
                    continue;
                };
                let total = info.get("total_token_usage").and_then(Raw::from_value);
                if let Some(t) = total {
                    if prev_total == Some(t) {
                        continue;
                    }
                }
                let last = info.get("last_token_usage").and_then(Raw::from_value);
                let raw = match (last, total) {
                    (Some(l), _) => l,
                    (None, Some(t)) => t.delta(&prev_total.unwrap_or_default()),
                    (None, None) => continue,
                };
                if total.is_some() {
                    prev_total = total;
                }
                let timestamp = v
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&Utc));
                let model = cur_model
                    .clone()
                    .or_else(|| meta_model.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                turns.push(ParsedTurn {
                    timestamp,
                    model,
                    usage: raw.into_usage(),
                    dedup_key: None,
                });
            }
            _ => {}
        }
    }

    Ok(ParsedSession {
        id: meta_id.unwrap_or_else(|| fallback_id(path)),
        source: Source::Codex,
        project: meta_cwd.or(ctx_cwd),
        file_path: path.to_string_lossy().into_owned(),
        turns,
        agent_id: None,
        parent_session_id,
        is_subagent,
        agent_label: agent_role.clone(),
        agent_type: agent_role,
        log_deleted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fx(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex")
            .join(name)
    }

    const NORMAL: &str = "rollout-2025-01-01T10-00-00-0199aaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl";
    const NO_META: &str =
        "no-meta-rollout-2025-01-06T10-00-00-0199aaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl";

    #[test]
    fn normal_session() {
        let s = parse_file(&fx(NORMAL)).unwrap();
        assert_eq!(s.id, "11111111-2222-3333-4444-555555555555");
        assert_eq!(s.source, Source::Codex);
        assert_eq!(s.project.as_deref(), Some("/home/dev/proj-a"));
        assert_eq!(s.turns.len(), 2);
        let t = &s.turns[0];
        assert_eq!(t.model, "gpt-5-codex");
        assert_eq!(t.usage.input, 800);
        assert_eq!(t.usage.cache_read, 200);
        assert_eq!(t.usage.output, 300);
        assert_eq!(t.usage.reasoning, 100);
        assert_eq!(t.usage.cache_creation, 0);
        assert!(t.dedup_key.is_none());
        assert_eq!(
            t.timestamp.unwrap().to_rfc3339(),
            "2025-01-01T10:00:05+00:00"
        );
        assert_eq!(s.turns[1].usage.input, 500);
        assert_eq!(s.turns[1].usage.cache_read, 1000);
    }

    #[test]
    fn null_info_skipped() {
        let s = parse_file(&fx("null-info.jsonl")).unwrap();
        assert_eq!(s.turns.len(), 1);
        assert_eq!(s.turns[0].usage.input, 100);
    }

    #[test]
    fn repeated_totals_skipped() {
        let s = parse_file(&fx("repeated.jsonl")).unwrap();
        assert_eq!(s.turns.len(), 2);
        assert_eq!(s.turns[1].usage.input, 100);
        assert_eq!(s.turns[1].usage.cache_read, 100);
        assert_eq!(s.turns[1].usage.output, 30);
    }

    #[test]
    fn missing_last_uses_delta() {
        let s = parse_file(&fx("no-last.jsonl")).unwrap();
        assert_eq!(s.turns.len(), 2);
        assert_eq!(s.turns[0].usage.input, 400);
        assert_eq!(s.turns[0].usage.cache_read, 100);
        assert_eq!(s.turns[1].usage.input, 200);
        assert_eq!(s.turns[1].usage.cache_read, 200);
        assert_eq!(s.turns[1].usage.output, 50);
        assert_eq!(s.turns[1].usage.reasoning, 10);
    }

    #[test]
    fn model_change_mid_session() {
        let s = parse_file(&fx("model-change.jsonl")).unwrap();
        assert_eq!(s.turns.len(), 2);
        assert_eq!(s.turns[0].model, "gpt-5");
        assert_eq!(s.turns[1].model, "gpt-5-codex");
    }

    #[test]
    fn missing_meta_id_from_filename() {
        let s = parse_file(&fx(NO_META)).unwrap();
        assert_eq!(s.id, "0199aaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert_eq!(s.project.as_deref(), Some("/home/dev/proj-f"));
        assert_eq!(s.turns.len(), 1);
    }

    #[test]
    fn non_uuid_stem_used_whole() {
        assert_eq!(fallback_id(Path::new("/x/foo.jsonl")), "foo");
    }

    #[test]
    fn bad_json_line_ignored() {
        let s = parse_file(&fx("bad-line.jsonl")).unwrap();
        assert_eq!(s.id, "aaaaaaaa-0000-0000-0000-000000000005");
        assert_eq!(s.turns.len(), 1);
    }

    #[test]
    fn missing_file_errs() {
        assert!(parse_file(&fx("nope.jsonl")).is_err());
    }
}
