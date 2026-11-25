use crate::model::{
    ParsedSession, ParsedTurn, SessionDetail, SessionSummary, Source, TokenUsage,
    TranscriptSummary, Turn,
};
use crate::parsers::{claude, codex};
use crate::pricing::PriceTable;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// Drop turns whose dedup key was already seen in an earlier session.
/// Sessions must be sorted oldest-first. Keyless turns are kept.
pub fn dedup_claude(sessions: &mut [ParsedSession]) {
    let mut seen: HashSet<String> = HashSet::new();
    for s in sessions.iter_mut() {
        // Keys first seen in this session only block later sessions, so
        // duplicates inside one session are the parser's concern.
        let mut new_keys = Vec::new();
        s.turns.retain(|t| match &t.dedup_key {
            Some(k) => {
                if seen.contains(k) {
                    false
                } else {
                    new_keys.push(k.clone());
                    true
                }
            }
            None => true,
        });
        seen.extend(new_keys);
    }
}

fn turn_cost(t: &ParsedTurn, prices: &PriceTable) -> Option<f64> {
    prices.cost(&t.model, &t.usage, t.timestamp)
}

pub fn summarize(s: &ParsedSession, prices: &PriceTable) -> SessionSummary {
    let models: BTreeSet<&str> = s.turns.iter().map(|t| t.model.as_str()).collect();
    let mut unpriced: BTreeSet<String> = BTreeSet::new();
    let mut usage = TokenUsage::default();
    let mut cost: Option<f64> = None;
    for t in &s.turns {
        usage.add(&t.usage);
        match turn_cost(t, prices) {
            Some(c) => *cost.get_or_insert(0.0) += c,
            None => {
                unpriced.insert(t.model.clone());
            }
        }
    }
    let stamps = || s.turns.iter().filter_map(|t| t.timestamp);
    let models: Vec<String> = models.into_iter().map(String::from).collect();
    let unpriced: Vec<String> = unpriced.into_iter().collect();
    let transcript = TranscriptSummary {
        file_path: s.file_path.clone(),
        agent_id: s.agent_id.clone(),
        agent_label: s.agent_label.clone(),
        agent_type: s.agent_type.clone(),
        is_subagent: s.is_subagent,
        log_deleted: s.log_deleted,
        models: models.clone(),
        started_at: stamps().min(),
        ended_at: stamps().max(),
        usage: usage.clone(),
        cost_usd: cost,
        unpriced_models: unpriced.clone(),
        turn_count: s.turns.len(),
    };
    SessionSummary {
        id: s.id.clone(),
        source: s.source,
        project: s.project.clone(),
        file_path: s.file_path.clone(),
        models,
        started_at: stamps().min(),
        ended_at: stamps().max(),
        usage,
        cost_usd: cost,
        unpriced_models: unpriced,
        turn_count: s.turns.len(),
        subagent_count: usize::from(s.is_subagent),
        log_deleted: s.log_deleted,
        transcripts: vec![transcript],
    }
}

pub fn detail(s: &ParsedSession, prices: &PriceTable) -> SessionDetail {
    SessionDetail {
        summary: summarize(s, prices),
        turns: s
            .turns
            .iter()
            .map(|t| Turn {
                timestamp: t.timestamp,
                model: t.model.clone(),
                usage: t.usage.clone(),
                cost_usd: turn_cost(t, prices),
                transcript: 0,
            })
            .collect(),
    }
}

fn parse_one(source: Source, path: &Path) -> Option<ParsedSession> {
    match source {
        Source::Claude => claude::parse_file(path).ok(),
        Source::Codex => codex::parse_file(path).ok(),
    }
}

/// Like `scan`, with a caller-supplied parser (lets callers add caching).
pub fn scan_with<F>(
    claude_files: &[PathBuf],
    codex_files: &[PathBuf],
    mut parse: F,
) -> Vec<ParsedSession>
where
    F: FnMut(Source, &Path) -> Option<ParsedSession>,
{
    let mut claude_sessions: Vec<ParsedSession> = claude_files
        .iter()
        .filter_map(|p| parse(Source::Claude, p))
        .filter(|s| !s.turns.is_empty())
        .collect();
    claude_sessions.sort_by(|a, b| {
        let first = |s: &ParsedSession| s.turns.iter().filter_map(|t| t.timestamp).min();
        first(a)
            .cmp(&first(b))
            .then_with(|| a.file_path.cmp(&b.file_path))
    });
    dedup_claude(&mut claude_sessions);
    claude_sessions.retain(|s| !s.turns.is_empty());

    let mut out = claude_sessions;
    out.extend(
        codex_files
            .iter()
            .filter_map(|p| parse(Source::Codex, p))
            .filter(|s| !s.turns.is_empty()),
    );
    out
}

pub fn scan(claude_files: &[PathBuf], codex_files: &[PathBuf]) -> Vec<ParsedSession> {
    scan_with(claude_files, codex_files, parse_one)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn turn(model: &str, key: Option<&str>, secs: i64, out: u64) -> ParsedTurn {
        ParsedTurn {
            timestamp: Some(Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()),
            model: model.into(),
            usage: TokenUsage {
                input: 1000,
                output: out,
                ..Default::default()
            },
            dedup_key: key.map(String::from),
        }
    }

    fn sess(path: &str, turns: Vec<ParsedTurn>) -> ParsedSession {
        ParsedSession {
            id: path.into(),
            source: Source::Claude,
            project: None,
            file_path: path.into(),
            turns,
            agent_id: None,
            is_subagent: false,
            agent_label: None,
            agent_type: None,
            log_deleted: false,
        }
    }

    #[test]
    fn dedup_across_sessions() {
        let mut v = vec![
            sess(
                "a",
                vec![turn("m", Some("k1"), 0, 1), turn("m", Some("k2"), 1, 1)],
            ),
            sess(
                "b",
                vec![turn("m", Some("k2"), 1, 1), turn("m", Some("k3"), 2, 1)],
            ),
        ];
        dedup_claude(&mut v);
        assert_eq!(v[0].turns.len(), 2);
        assert_eq!(v[1].turns.len(), 1);
        assert_eq!(v[1].turns[0].dedup_key.as_deref(), Some("k3"));
    }

    #[test]
    fn keyless_turns_kept() {
        let mut v = vec![
            sess("a", vec![turn("m", None, 0, 1)]),
            sess("b", vec![turn("m", None, 1, 1)]),
        ];
        dedup_claude(&mut v);
        assert_eq!(v[0].turns.len(), 1);
        assert_eq!(v[1].turns.len(), 1);
    }

    fn table() -> PriceTable {
        // Bundled prices: claude-sonnet-4-5 priced, made-up model unpriced.
        PriceTable::load(Path::new("/nonexistent-cache-dir"))
    }

    #[test]
    fn summarize_partial_cost() {
        let p = table();
        assert!(p.price_for("claude-sonnet-4-5", None).is_some());
        assert!(p.price_for("zzz-unknown-model", None).is_none());
        let s = sess(
            "a",
            vec![
                turn("zzz-unknown-model", None, 5, 7),
                turn("claude-sonnet-4-5", None, 1, 3),
                turn("claude-sonnet-4-5", None, 3, 3),
            ],
        );
        let sum = summarize(&s, &p);
        assert_eq!(sum.turn_count, 3);
        assert_eq!(sum.models, vec!["claude-sonnet-4-5", "zzz-unknown-model"]);
        assert_eq!(sum.unpriced_models, vec!["zzz-unknown-model"]);
        assert_eq!(sum.usage.input, 3000);
        assert_eq!(sum.usage.output, 13);
        assert!(sum.started_at < sum.ended_at);
        let one = p
            .cost("claude-sonnet-4-5", &s.turns[1].usage, s.turns[1].timestamp)
            .unwrap();
        assert!((sum.cost_usd.unwrap() - 2.0 * one).abs() < 1e-12);
        let d = detail(&s, &p);
        assert!(d.turns[0].cost_usd.is_none());
        assert!(d.turns[1].cost_usd.is_some());
    }

    #[test]
    fn all_unpriced_is_none() {
        let s = sess("a", vec![turn("zzz-unknown-model", None, 0, 1)]);
        let sum = summarize(&s, &table());
        assert!(sum.cost_usd.is_none());
        assert_eq!(sum.unpriced_models, vec!["zzz-unknown-model"]);
    }

    #[test]
    fn scan_fixtures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let c = crate::discovery::find_jsonl(&[root.join("claude")]);
        let x = crate::discovery::find_jsonl(&[root.join("codex")]);
        assert!(!c.is_empty() && !x.is_empty());
        let all = scan(&c, &x);
        assert!(all.iter().any(|s| s.source == Source::Claude));
        assert!(all.iter().any(|s| s.source == Source::Codex));
        assert!(all.iter().all(|s| !s.turns.is_empty()));
        let p = table();
        let total: u64 = all.iter().map(|s| summarize(s, &p).usage.total()).sum();
        assert!(total > 0);
        // no dedup key repeats across Claude sessions
        let mut seen = HashSet::new();
        for s in all.iter().filter(|s| s.source == Source::Claude) {
            let own: HashSet<_> = s.turns.iter().filter_map(|t| t.dedup_key.clone()).collect();
            assert!(seen.is_disjoint(&own));
            seen.extend(own);
        }
    }
}
