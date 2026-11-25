use crate::model::{
    ParsedSession, ParsedTurn, SessionDetail, SessionSummary, Source, TokenUsage,
    TranscriptSummary, Turn,
};
use crate::pricing::PriceTable;
use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashMap, HashSet};

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

/// All transcripts (main + subagents) sharing one (source, id).
#[derive(Debug, Clone)]
pub struct SessionGroup {
    pub source: Source,
    pub id: String,
    pub transcripts: Vec<ParsedSession>,
}

fn first_stamp(s: &ParsedSession) -> Option<DateTime<Utc>> {
    s.turns.iter().filter_map(|t| t.timestamp).min()
}

/// Group by (source, id). Order inside a group: mains first, then
/// subagents by first turn timestamp (None last), ties by file path.
/// Groups keep first-seen order.
pub fn group(sessions: Vec<ParsedSession>) -> Vec<SessionGroup> {
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    let mut groups: Vec<SessionGroup> = Vec::new();
    for s in sessions {
        let src = match s.source {
            Source::Claude => "claude",
            Source::Codex => "codex",
        };
        let key = (src.to_string(), s.id.clone());
        let i = *index.entry(key).or_insert_with(|| {
            groups.push(SessionGroup {
                source: s.source,
                id: s.id.clone(),
                transcripts: Vec::new(),
            });
            groups.len() - 1
        });
        groups[i].transcripts.push(s);
    }
    for g in &mut groups {
        g.transcripts.sort_by(|a, b| {
            a.is_subagent
                .cmp(&b.is_subagent)
                .then_with(|| {
                    let (fa, fb) = (first_stamp(a), first_stamp(b));
                    fa.is_none().cmp(&fb.is_none()).then_with(|| fa.cmp(&fb))
                })
                .then_with(|| a.file_path.cmp(&b.file_path))
        });
    }
    groups
}

fn transcript_summary(s: &ParsedSession, prices: &PriceTable) -> TranscriptSummary {
    let one = summarize(s, prices);
    one.transcripts
        .into_iter()
        .next()
        .expect("summarize yields one transcript")
}

pub fn summarize_group(g: &SessionGroup, prices: &PriceTable) -> SessionSummary {
    let transcripts: Vec<TranscriptSummary> = g
        .transcripts
        .iter()
        .map(|s| transcript_summary(s, prices))
        .collect();
    let main = g.transcripts.iter().find(|s| !s.is_subagent);
    let file_path = main
        .or(g.transcripts.first())
        .map(|s| s.file_path.clone())
        .unwrap_or_default();
    let project = match main {
        Some(m) => m.project.clone(),
        None => g.transcripts.iter().find_map(|s| s.project.clone()),
    };
    let mut models: BTreeSet<String> = BTreeSet::new();
    let mut unpriced: BTreeSet<String> = BTreeSet::new();
    let mut usage = TokenUsage::default();
    let mut cost: Option<f64> = None;
    for t in &transcripts {
        models.extend(t.models.iter().cloned());
        unpriced.extend(t.unpriced_models.iter().cloned());
        usage.add(&t.usage);
        if let Some(c) = t.cost_usd {
            *cost.get_or_insert(0.0) += c;
        }
    }
    SessionSummary {
        id: g.id.clone(),
        source: g.source,
        project,
        file_path,
        models: models.into_iter().collect(),
        started_at: transcripts.iter().filter_map(|t| t.started_at).min(),
        ended_at: transcripts.iter().filter_map(|t| t.ended_at).max(),
        usage,
        cost_usd: cost,
        unpriced_models: unpriced.into_iter().collect(),
        turn_count: transcripts.iter().map(|t| t.turn_count).sum(),
        subagent_count: transcripts.iter().filter(|t| t.is_subagent).count(),
        log_deleted: !transcripts.is_empty() && transcripts.iter().all(|t| t.log_deleted),
        transcripts,
    }
}

pub fn detail_group(g: &SessionGroup, prices: &PriceTable) -> SessionDetail {
    let mut turns: Vec<Turn> = g
        .transcripts
        .iter()
        .enumerate()
        .flat_map(|(i, s)| {
            s.turns.iter().map(move |t| Turn {
                timestamp: t.timestamp,
                model: t.model.clone(),
                usage: t.usage.clone(),
                cost_usd: turn_cost(t, prices),
                transcript: i,
            })
        })
        .collect();
    turns.sort_by(|a, b| {
        a.timestamp
            .is_none()
            .cmp(&b.timestamp.is_none())
            .then_with(|| a.timestamp.cmp(&b.timestamp))
    });
    SessionDetail {
        summary: summarize_group(g, prices),
        turns,
    }
}

/// Dedup Claude transcripts (oldest-first), drop empties, group, and
/// summarize. Summaries sorted by started_at desc, None last.
pub fn build(
    sessions: Vec<ParsedSession>,
    prices: &PriceTable,
) -> (Vec<SessionGroup>, Vec<SessionSummary>) {
    let (mut claude_s, other): (Vec<_>, Vec<_>) = sessions
        .into_iter()
        .partition(|s| s.source == Source::Claude);
    claude_s.sort_by(|a, b| {
        first_stamp(a)
            .cmp(&first_stamp(b))
            .then_with(|| a.file_path.cmp(&b.file_path))
    });
    dedup_claude(&mut claude_s);
    let all: Vec<ParsedSession> = claude_s
        .into_iter()
        .chain(other)
        .filter(|s| !s.turns.is_empty())
        .collect();
    let mut pairs: Vec<(SessionGroup, SessionSummary)> = group(all)
        .into_iter()
        .map(|g| {
            let s = summarize_group(&g, prices);
            (g, s)
        })
        .collect();
    pairs.sort_by(|(_, a), (_, b)| {
        a.started_at
            .is_none()
            .cmp(&b.started_at.is_none())
            .then_with(|| b.started_at.cmp(&a.started_at))
    });
    pairs.into_iter().unzip()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::path::Path;

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
    }

    #[test]
    fn all_unpriced_is_none() {
        let s = sess("a", vec![turn("zzz-unknown-model", None, 0, 1)]);
        let sum = summarize(&s, &table());
        assert!(sum.cost_usd.is_none());
        assert_eq!(sum.unpriced_models, vec!["zzz-unknown-model"]);
    }

    fn parse_fixtures(dir: &str, src: Source) -> Vec<ParsedSession> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let files = crate::discovery::find_jsonl(&[root.join(dir)]);
        assert!(!files.is_empty());
        files
            .iter()
            .map(|p| match src {
                Source::Claude => crate::parsers::claude::parse_file(p).unwrap(),
                Source::Codex => crate::parsers::codex::parse_file(p).unwrap(),
            })
            .collect()
    }

    #[test]
    fn build_fixtures() {
        let mut all = parse_fixtures("claude", Source::Claude);
        all.extend(parse_fixtures("codex", Source::Codex));
        let (groups, sums) = build(all, &table());
        assert_eq!(groups.len(), sums.len());
        assert!(sums.iter().any(|s| s.source == Source::Claude));
        assert!(sums.iter().any(|s| s.source == Source::Codex));
        assert!(groups
            .iter()
            .flat_map(|g| &g.transcripts)
            .all(|s| !s.turns.is_empty()));
        assert!(sums.iter().map(|s| s.usage.total()).sum::<u64>() > 0);
        // no dedup key repeats across Claude transcripts
        let mut seen = HashSet::new();
        for s in groups
            .iter()
            .flat_map(|g| &g.transcripts)
            .filter(|s| s.source == Source::Claude)
        {
            let own: HashSet<_> = s.turns.iter().filter_map(|t| t.dedup_key.clone()).collect();
            assert!(seen.is_disjoint(&own));
            seen.extend(own);
        }
    }

    fn sub(id: &str, path: &str, agent: &str, turns: Vec<ParsedTurn>) -> ParsedSession {
        let mut s = sess(path, turns);
        s.id = id.into();
        s.is_subagent = true;
        s.agent_id = Some(agent.into());
        s
    }

    fn main_s(id: &str, path: &str, turns: Vec<ParsedTurn>) -> ParsedSession {
        let mut s = sess(path, turns);
        s.id = id.into();
        s.project = Some("proj".into());
        s
    }

    const M: &str = "claude-sonnet-4-5";

    #[test]
    fn group_main_and_subagents() {
        let p = table();
        let v = vec![
            sub("s", "sub-late", "b", vec![turn(M, Some("b1"), 50, 4)]),
            main_s(
                "s",
                "main",
                vec![turn(M, Some("m1"), 10, 2), turn(M, Some("m2"), 60, 2)],
            ),
            sub(
                "s",
                "sub-early",
                "a",
                vec![turn("zzz-unknown-model", Some("a1"), 20, 3)],
            ),
            main_s("other", "o", vec![turn(M, Some("o1"), 5, 1)]),
        ];
        let g = group(v);
        assert_eq!(g.len(), 2);
        let paths: Vec<_> = g[0]
            .transcripts
            .iter()
            .map(|t| t.file_path.as_str())
            .collect();
        assert_eq!(paths, ["main", "sub-early", "sub-late"]);
        let sum = summarize_group(&g[0], &p);
        assert_eq!(sum.file_path, "main");
        assert_eq!(sum.project.as_deref(), Some("proj"));
        assert_eq!(sum.subagent_count, 2);
        assert_eq!(sum.turn_count, 4);
        assert_eq!(sum.transcripts.len(), 3);
        assert_eq!(sum.models, vec![M, "zzz-unknown-model"]);
        assert_eq!(sum.unpriced_models, vec!["zzz-unknown-model"]);
        assert_eq!(sum.usage.output, 2 + 2 + 3 + 4);
        assert_eq!(sum.usage.input, 4000);
        let parts: f64 = sum.transcripts.iter().filter_map(|t| t.cost_usd).sum();
        assert!((sum.cost_usd.unwrap() - parts).abs() < 1e-12);
        assert!(sum.transcripts[1].cost_usd.is_none());
        assert!(sum.transcripts[0].cost_usd.is_some());
        assert!(!sum.transcripts[0].is_subagent && sum.transcripts[2].is_subagent);
        assert_eq!(sum.transcripts[2].agent_id.as_deref(), Some("b"));
        assert_eq!(sum.started_at, g[0].transcripts[0].turns[0].timestamp);
        assert_eq!(sum.ended_at, g[0].transcripts[0].turns[1].timestamp);
    }

    #[test]
    fn subagent_only_group_fallback() {
        let mut a = sub("s", "z-path", "a", vec![turn(M, None, 10, 1)]);
        a.project = Some("pa".into());
        let b = sub("s", "a-path", "b", vec![turn(M, None, 10, 1)]);
        let g = group(vec![a, b]);
        let sum = summarize_group(&g[0], &table());
        assert_eq!(sum.file_path, "a-path");
        assert_eq!(sum.project.as_deref(), Some("pa"));
        assert_eq!(sum.subagent_count, 2);
    }

    #[test]
    fn log_deleted_all_vs_partial() {
        let mk = |d1: bool, d2: bool| {
            let mut a = main_s("s", "m", vec![turn(M, None, 0, 1)]);
            let mut b = sub("s", "sa", "a", vec![turn(M, None, 1, 1)]);
            a.log_deleted = d1;
            b.log_deleted = d2;
            group(vec![a, b])
        };
        let p = table();
        assert!(summarize_group(&mk(true, true)[0], &p).log_deleted);
        let part = summarize_group(&mk(true, false)[0], &p);
        assert!(!part.log_deleted);
        assert!(part.transcripts[0].log_deleted && !part.transcripts[1].log_deleted);
    }

    #[test]
    fn detail_group_orders_turns() {
        let mut none_turn = turn(M, None, 0, 9);
        none_turn.timestamp = None;
        let v = vec![
            main_s("s", "m", vec![turn(M, None, 10, 1), turn(M, None, 30, 1)]),
            sub("s", "sa", "a", vec![turn(M, None, 20, 1), none_turn]),
        ];
        let g = group(v);
        let d = detail_group(&g[0], &table());
        let idx: Vec<usize> = d.turns.iter().map(|t| t.transcript).collect();
        assert_eq!(idx, [0, 1, 0, 1]);
        assert!(d.turns[3].timestamp.is_none());
        assert!(d.turns[0].timestamp < d.turns[1].timestamp);
        assert_eq!(d.summary.turn_count, 4);
        assert!(d.turns.iter().all(|t| t.cost_usd.is_some()));
    }

    #[test]
    fn build_dedups_mains_keeps_subagents() {
        let shared = turn(M, Some("k1"), 1, 1);
        let v = vec![
            main_s(
                "s2",
                "resumed",
                vec![shared.clone(), turn(M, Some("k2"), 100, 1)],
            ),
            main_s("s1", "orig", vec![shared.clone()]),
            sub("s2", "sa", "a", vec![turn(M, Some("sa1"), 101, 1)]),
            main_s("s3", "zz-dupe", vec![shared]),
        ];
        let (groups, sums) = build(v, &table());
        // s3 fully duplicated (path sorts last on timestamp tie) -> dropped
        let ids: Vec<_> = sums.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["s2", "s1"]);
        assert_eq!(groups.len(), 2);
        let s2 = &sums[0];
        assert_eq!(s2.turn_count, 2);
        assert_eq!(s2.subagent_count, 1);
        assert_eq!(sums[1].turn_count, 1);
    }

    #[test]
    fn build_sort_started_desc_none_last() {
        let mut nots = turn(M, None, 0, 1);
        nots.timestamp = None;
        let v = vec![
            main_s("old", "a", vec![turn(M, None, 1, 1)]),
            main_s("none", "b", vec![nots]),
            main_s("new", "c", vec![turn(M, None, 500, 1)]),
            main_s("empty", "d", vec![]),
        ];
        let (g, sums) = build(v, &table());
        let ids: Vec<_> = sums.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["new", "old", "none"]);
        assert_eq!(g.len(), 3);
    }

    #[test]
    fn codex_single_transcript_group() {
        let mut c = sess("rollout", vec![turn("gpt-5", None, 0, 5)]);
        c.source = Source::Codex;
        c.id = "x".into();
        let (g, sums) = build(vec![c], &table());
        assert_eq!(g[0].transcripts.len(), 1);
        assert_eq!(sums[0].subagent_count, 0);
        assert_eq!(sums[0].transcripts.len(), 1);
        assert_eq!(sums[0].source, Source::Codex);
    }

    #[test]
    fn build_fixtures_groups_subagents() {
        let all = parse_fixtures("claude", Source::Claude);
        let (groups, sums) = build(all, &table());
        assert_eq!(groups.len(), sums.len());
        let with_sub: Vec<_> = sums.iter().filter(|s| s.subagent_count > 0).collect();
        assert!(!with_sub.is_empty());
        for s in with_sub {
            assert_eq!(
                s.transcripts.len() - s.subagent_count,
                s.transcripts.iter().filter(|t| !t.is_subagent).count()
            );
        }
    }
}
