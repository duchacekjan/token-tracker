//! SQLite store of parsed transcripts. Tokens only; cost is computed at read time.

use crate::model::{ParsedSession, ParsedTurn, Source, TokenUsage};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Bump when parser output semantics change; forces a reparse of every file.
pub const PARSER_VERSION: i64 = 1;

const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncStats {
    pub parsed: usize,
    pub unchanged: usize,
    pub failed: usize,
    /// Stored files that are no longer on disk (data kept, `present = 0`).
    pub missing: usize,
}

pub struct Store {
    conn: Connection,
}

fn source_str(s: Source) -> &'static str {
    match s {
        Source::Claude => "claude",
        Source::Codex => "codex",
    }
}

fn source_from(s: &str) -> anyhow::Result<Source> {
    match s {
        "claude" => Ok(Source::Claude),
        "codex" => Ok(Source::Codex),
        other => anyhow::bail!("unknown source in db: {other}"),
    }
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn to_u64(v: Option<i64>) -> u64 {
    v.and_then(|x| u64::try_from(x).ok()).unwrap_or(0)
}

/// (size, mtime ns since epoch) of a file.
fn stat(path: &Path) -> std::io::Result<(i64, i64)> {
    let md = std::fs::metadata(path)?;
    let mtime = md.modified()?;
    let ns = match mtime.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_nanos()).unwrap_or(i64::MAX),
    };
    Ok((to_i64(md.len()), ns))
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> anyhow::Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            anyhow::bail!("db schema version {version} is newer than supported {SCHEMA_VERSION}");
        }
        if version < 1 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(
                "CREATE TABLE files (
                    path TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    mtime_ns INTEGER NOT NULL,
                    parser_version INTEGER NOT NULL,
                    session_id TEXT NOT NULL,
                    project TEXT,
                    agent_id TEXT,
                    is_subagent INTEGER NOT NULL,
                    agent_label TEXT,
                    agent_type TEXT,
                    present INTEGER NOT NULL,
                    parsed_at TEXT NOT NULL
                );
                CREATE TABLE turns (
                    path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
                    seq INTEGER NOT NULL,
                    timestamp TEXT,
                    model TEXT NOT NULL,
                    input INTEGER,
                    output INTEGER,
                    cache_creation INTEGER,
                    cache_read INTEGER,
                    reasoning INTEGER,
                    dedup_key TEXT,
                    PRIMARY KEY(path, seq)
                );
                CREATE INDEX idx_files_source_session ON files(source, session_id);
                PRAGMA user_version = 1;",
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn sync<F>(
        &mut self,
        claude_files: &[PathBuf],
        codex_files: &[PathBuf],
        parse: F,
    ) -> anyhow::Result<SyncStats>
    where
        F: Fn(Source, &Path) -> anyhow::Result<ParsedSession>,
    {
        let mut stats = SyncStats::default();
        let tx = self.conn.transaction()?;
        let mut seen: HashSet<String> = HashSet::new();

        let listed = claude_files
            .iter()
            .map(|p| (Source::Claude, p))
            .chain(codex_files.iter().map(|p| (Source::Codex, p)));

        for (source, path) in listed {
            let key = path.to_string_lossy().into_owned();
            if !seen.insert(key.clone()) {
                continue;
            }
            let Ok((size, mtime_ns)) = stat(path) else {
                // Unreadable/vanished: handled by the missing pass below.
                continue;
            };

            let existing: Option<(i64, i64, i64)> = tx
                .query_row(
                    "SELECT size, mtime_ns, parser_version FROM files WHERE path = ?1",
                    [&key],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            if existing == Some((size, mtime_ns, PARSER_VERSION)) {
                tx.execute("UPDATE files SET present = 1 WHERE path = ?1", [&key])?;
                stats.unchanged += 1;
                continue;
            }

            let session = match parse(source, path) {
                Ok(s) => s,
                Err(_) => {
                    stats.failed += 1;
                    continue;
                }
            };

            // Replace row + turns (cascade removes old turns).
            tx.execute("DELETE FROM files WHERE path = ?1", [&key])?;
            tx.execute(
                "INSERT INTO files (path, source, size, mtime_ns, parser_version, session_id,
                    project, agent_id, is_subagent, agent_label, agent_type, present, parsed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12)",
                params![
                    key,
                    source_str(source),
                    size,
                    mtime_ns,
                    PARSER_VERSION,
                    session.id,
                    session.project,
                    session.agent_id,
                    session.is_subagent,
                    session.agent_label,
                    session.agent_type,
                    Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )?;
            {
                let mut ins = tx.prepare(
                    "INSERT INTO turns (path, seq, timestamp, model, input, output,
                        cache_creation, cache_read, reasoning, dedup_key)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )?;
                for (seq, t) in session.turns.iter().enumerate() {
                    ins.execute(params![
                        key,
                        seq as i64,
                        t.timestamp
                            .map(|ts| ts.to_rfc3339_opts(SecondsFormat::Nanos, true)),
                        t.model,
                        to_i64(t.usage.input),
                        to_i64(t.usage.output),
                        to_i64(t.usage.cache_creation),
                        to_i64(t.usage.cache_read),
                        to_i64(t.usage.reasoning),
                        t.dedup_key,
                    ])?;
                }
            }
            stats.parsed += 1;
        }

        // Missing pass: stored paths absent from the lists and from disk, or listed but unstat-able.
        let stored: Vec<(String, bool)> = {
            let mut q = tx.prepare("SELECT path, present FROM files")?;
            let rows = q.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)))?;
            rows.collect::<Result<_, _>>()?
        };
        for (path, was_present) in stored {
            let on_disk = stat(Path::new(&path)).is_ok();
            if !on_disk {
                if was_present {
                    tx.execute("UPDATE files SET present = 0 WHERE path = ?1", [&path])?;
                }
                stats.missing += 1;
            }
        }

        tx.commit()?;
        Ok(stats)
    }

    pub fn load_all(&self) -> anyhow::Result<Vec<ParsedSession>> {
        struct Row {
            path: String,
            source: String,
            id: String,
            project: Option<String>,
            agent_id: Option<String>,
            is_subagent: bool,
            agent_label: Option<String>,
            agent_type: Option<String>,
            present: bool,
        }
        let rows: Vec<Row> = {
            let mut q = self.conn.prepare(
                "SELECT path, source, session_id, project, agent_id, is_subagent,
                        agent_label, agent_type, present
                 FROM files ORDER BY path",
            )?;
            let it = q.query_map([], |r| {
                Ok(Row {
                    path: r.get(0)?,
                    source: r.get(1)?,
                    id: r.get(2)?,
                    project: r.get(3)?,
                    agent_id: r.get(4)?,
                    is_subagent: r.get::<_, i64>(5)? != 0,
                    agent_label: r.get(6)?,
                    agent_type: r.get(7)?,
                    present: r.get::<_, i64>(8)? != 0,
                })
            })?;
            it.collect::<Result<_, _>>()?
        };

        let mut turn_q = self.conn.prepare(
            "SELECT timestamp, model, input, output, cache_creation, cache_read, reasoning, dedup_key
             FROM turns WHERE path = ?1 ORDER BY seq",
        )?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let turns = turn_q
                .query_map([&row.path], |r| {
                    let ts: Option<String> = r.get(0)?;
                    Ok(ParsedTurn {
                        timestamp: ts
                            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                            .map(|d| d.with_timezone(&Utc)),
                        model: r.get(1)?,
                        usage: TokenUsage {
                            input: to_u64(r.get(2)?),
                            output: to_u64(r.get(3)?),
                            cache_creation: to_u64(r.get(4)?),
                            cache_read: to_u64(r.get(5)?),
                            reasoning: to_u64(r.get(6)?),
                        },
                        dedup_key: r.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            out.push(ParsedSession {
                id: row.id,
                source: source_from(&row.source)?,
                project: row.project,
                file_path: row.path,
                turns,
                agent_id: row.agent_id,
                is_subagent: row.is_subagent,
                agent_label: row.agent_label,
                agent_type: row.agent_type,
                log_deleted: !row.present,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::{claude, codex};
    use std::cell::Cell;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn real_parse(source: Source, path: &Path) -> anyhow::Result<ParsedSession> {
        match source {
            Source::Claude => claude::parse_file(path),
            Source::Codex => codex::parse_file(path),
        }
    }

    /// Copy fixtures into a temp dir; returns (claude files, codex files).
    fn setup(dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let c = dir.join("claude");
        let x = dir.join("codex");
        std::fs::create_dir_all(&c).unwrap();
        std::fs::create_dir_all(&x).unwrap();
        let mut cf = Vec::new();
        for n in ["normal.jsonl", "duplicates.jsonl"] {
            std::fs::copy(fixtures().join("claude").join(n), c.join(n)).unwrap();
            cf.push(c.join(n));
        }
        let sub = fixtures()
            .join("claude/11111111-2222-4333-8444-555555555555/subagents/agent-abc.jsonl");
        let sub_dst_dir = c.join("11111111-2222-4333-8444-555555555555/subagents");
        std::fs::create_dir_all(&sub_dst_dir).unwrap();
        std::fs::copy(&sub, sub_dst_dir.join("agent-abc.jsonl")).unwrap();
        std::fs::copy(
            sub.with_file_name("agent-abc.meta.json"),
            sub_dst_dir.join("agent-abc.meta.json"),
        )
        .unwrap();
        cf.push(sub_dst_dir.join("agent-abc.jsonl"));
        let xn = "rollout-2025-01-01T10-00-00-0199aaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl";
        std::fs::copy(fixtures().join("codex").join(xn), x.join(xn)).unwrap();
        (cf, vec![x.join(xn)])
    }

    fn sorted(mut v: Vec<ParsedSession>) -> Vec<ParsedSession> {
        v.sort_by(|a, b| a.file_path.cmp(&b.file_path));
        v
    }

    fn same(a: &ParsedSession, b: &ParsedSession) -> bool {
        serde_json::to_value(a).unwrap() == serde_json::to_value(b).unwrap()
    }

    #[test]
    fn first_sync_parses_all_then_unchanged() {
        let td = tempfile::tempdir().unwrap();
        let (cf, xf) = setup(td.path());
        let mut s = Store::open_in_memory().unwrap();
        let st = s.sync(&cf, &xf, real_parse).unwrap();
        assert_eq!(
            st,
            SyncStats {
                parsed: 4,
                unchanged: 0,
                failed: 0,
                missing: 0
            }
        );

        let calls = Cell::new(0);
        let st = s
            .sync(&cf, &xf, |src, p| {
                calls.set(calls.get() + 1);
                real_parse(src, p)
            })
            .unwrap();
        assert_eq!(calls.get(), 0);
        assert_eq!(st.unchanged, 4);
        assert_eq!(st.parsed, 0);
    }

    #[test]
    fn roundtrip_all_fields() {
        let td = tempfile::tempdir().unwrap();
        let (cf, xf) = setup(td.path());
        let mut s = Store::open_in_memory().unwrap();
        s.sync(&cf, &xf, real_parse).unwrap();
        let loaded = sorted(s.load_all().unwrap());
        assert_eq!(loaded.len(), 4);
        let mut expected: Vec<ParsedSession> = cf
            .iter()
            .map(|p| claude::parse_file(p).unwrap())
            .chain(xf.iter().map(|p| codex::parse_file(p).unwrap()))
            .collect();
        expected = sorted(expected);
        for (a, b) in loaded.iter().zip(expected.iter()) {
            assert!(same(a, b), "mismatch {a:?} vs {b:?}");
            assert!(!a.log_deleted);
        }
        let sub = loaded.iter().find(|x| x.is_subagent).expect("subagent");
        assert!(sub.agent_id.is_some());
        assert!(sub.agent_label.is_some() || sub.agent_type.is_some());
        assert!(loaded.iter().any(|x| x.source == Source::Codex));
        assert!(loaded.iter().any(|x| !x.turns.is_empty()));
    }

    #[test]
    fn changed_file_reparsed_rows_replaced() {
        let td = tempfile::tempdir().unwrap();
        let (cf, xf) = setup(td.path());
        let mut s = Store::open_in_memory().unwrap();
        s.sync(&cf, &xf, real_parse).unwrap();
        let target = cf[0].clone();
        let before = s
            .load_all()
            .unwrap()
            .into_iter()
            .find(|x| Path::new(&x.file_path) == target)
            .unwrap()
            .turns
            .len();

        let line = r#"{"type":"assistant","sessionId":"s","timestamp":"2025-02-01T00:00:00Z","message":{"id":"msg_new","model":"claude-sonnet-4-5","usage":{"input_tokens":7,"output_tokens":9}},"requestId":"req_new"}"#;
        let mut content = std::fs::read_to_string(&target).unwrap();
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(line);
        content.push('\n');
        std::fs::write(&target, content).unwrap();

        let st = s.sync(&cf, &xf, real_parse).unwrap();
        assert_eq!(st.parsed, 1);
        assert_eq!(st.unchanged, 3);
        let after = s
            .load_all()
            .unwrap()
            .into_iter()
            .find(|x| Path::new(&x.file_path) == target)
            .unwrap();
        assert_eq!(after.turns.len(), before + 1);
        let n: i64 = s
            .conn
            .query_row(
                "SELECT COUNT(*) FROM turns WHERE path = ?1",
                [target.to_string_lossy().as_ref()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n as usize, before + 1);
    }

    #[test]
    fn deleted_file_kept_and_marked() {
        let td = tempfile::tempdir().unwrap();
        let (cf, xf) = setup(td.path());
        let mut s = Store::open_in_memory().unwrap();
        s.sync(&cf, &xf, real_parse).unwrap();
        let before = sorted(s.load_all().unwrap());

        std::fs::remove_file(&cf[0]).unwrap();
        // Dropped from list.
        let st = s.sync(&cf[1..], &xf, real_parse).unwrap();
        assert_eq!(st.missing, 1);
        assert_eq!(st.unchanged, 3);
        let after = sorted(s.load_all().unwrap());
        assert_eq!(after.len(), 4);
        let gone = after
            .iter()
            .find(|x| Path::new(&x.file_path) == cf[0])
            .unwrap();
        assert!(gone.log_deleted);
        let orig = before
            .iter()
            .find(|x| Path::new(&x.file_path) == cf[0])
            .unwrap();
        assert_eq!(gone.turns.len(), orig.turns.len());
        assert_eq!(after.iter().filter(|x| x.log_deleted).count(), 1);

        // Still listed but missing: also marked.
        let st = s.sync(&cf, &xf, real_parse).unwrap();
        assert_eq!(st.missing, 1);

        // Reappears: present again.
        std::fs::copy(fixtures().join("claude/normal.jsonl"), &cf[0]).unwrap();
        let st = s.sync(&cf, &xf, real_parse).unwrap();
        assert_eq!(st.missing, 0);
        assert!(s.load_all().unwrap().iter().all(|x| !x.log_deleted));
    }

    #[test]
    fn parse_error_keeps_old_data() {
        let td = tempfile::tempdir().unwrap();
        let (cf, xf) = setup(td.path());
        let mut s = Store::open_in_memory().unwrap();
        s.sync(&cf, &xf, real_parse).unwrap();
        let before = sorted(s.load_all().unwrap());

        let mut content = std::fs::read_to_string(&cf[0]).unwrap();
        content.push_str("\n\n");
        std::fs::write(&cf[0], content).unwrap();

        let st = s
            .sync(&cf, &xf, |src, p| {
                if p == cf[0] {
                    anyhow::bail!("boom")
                }
                real_parse(src, p)
            })
            .unwrap();
        assert_eq!(st.failed, 1);
        assert_eq!(st.unchanged, 3);
        let after = sorted(s.load_all().unwrap());
        assert_eq!(after.len(), before.len());
        for (a, b) in after.iter().zip(before.iter()) {
            assert!(same(a, b));
        }
        // Retries next sync.
        let st = s.sync(&cf, &xf, real_parse).unwrap();
        assert_eq!(st.parsed, 1);
        assert_eq!(st.failed, 0);
    }

    #[test]
    fn parser_version_bump_forces_reparse() {
        let td = tempfile::tempdir().unwrap();
        let (cf, xf) = setup(td.path());
        let mut s = Store::open_in_memory().unwrap();
        s.sync(&cf, &xf, real_parse).unwrap();
        s.conn
            .execute("UPDATE files SET parser_version = parser_version - 1", [])
            .unwrap();
        let st = s.sync(&cf, &xf, real_parse).unwrap();
        assert_eq!(st.parsed, 4);
        assert_eq!(st.unchanged, 0);
    }

    #[test]
    fn reopen_from_disk_keeps_data() {
        let td = tempfile::tempdir().unwrap();
        let (cf, xf) = setup(td.path());
        let db = td.path().join("nested/dir/tt.db");
        {
            let mut s = Store::open(&db).unwrap();
            s.sync(&cf, &xf, real_parse).unwrap();
        }
        let mut s = Store::open(&db).unwrap();
        assert_eq!(s.load_all().unwrap().len(), 4);
        let calls = Cell::new(0);
        let st = s
            .sync(&cf, &xf, |src, p| {
                calls.set(calls.get() + 1);
                real_parse(src, p)
            })
            .unwrap();
        assert_eq!(calls.get(), 0);
        assert_eq!(st.unchanged, 4);
    }
}
