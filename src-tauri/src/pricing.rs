use crate::model::TokenUsage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// USD per token.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_creation: f64,
    pub cache_read: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingStatus {
    /// "online" | "cache" | "bundled"
    pub source: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub model_count: usize,
    pub error: Option<String>,
}

const BUNDLED: &str = include_str!("../resources/price_history.json");
const PRICES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const LOCAL_FILE: &str = "price_history_local.json";
const TRACKED_PREFIXES: [&str; 10] = [
    "claude-", "gpt-5", "gpt-4.1", "gpt-4o", "o1", "o3", "o4-mini", "codex-", "gpt-6", "gemini-",
];
const PROVIDER_PREFIX: &str = "openai.";

/// One price valid from `from` until the next entry. `None` cache = input price.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Entry {
    from: DateTime<Utc>,
    input: f64,
    output: f64,
    cache_creation: Option<f64>,
    cache_read: Option<f64>,
}

impl Entry {
    fn price(&self) -> ModelPrice {
        ModelPrice {
            input: self.input,
            output: self.output,
            cache_creation: self.cache_creation.unwrap_or(self.input),
            cache_read: self.cache_read.unwrap_or(self.input),
        }
    }
}

type Models = BTreeMap<String, Vec<Entry>>;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct History {
    #[serde(default)]
    source: String,
    #[serde(default)]
    generated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    head_commit: String,
    #[serde(default)]
    models: Models,
}

pub struct PriceTable {
    /// Keys lowercased; entries sorted by `from`.
    models: Models,
    status: PricingStatus,
}

/// Union entries per model, sort by `from`, drop entries whose price equals the previous one.
fn merge(a: &Models, b: &Models) -> Models {
    let mut out: Models = BTreeMap::new();
    for (k, v) in a.iter().chain(b.iter()) {
        out.entry(k.to_lowercase())
            .or_default()
            .extend(v.iter().copied());
    }
    for v in out.values_mut() {
        v.sort_by_key(|e| e.from);
        let mut kept: Vec<Entry> = Vec::with_capacity(v.len());
        for e in v.iter() {
            if kept.last().map(|l| l.price()) != Some(e.price()) {
                kept.push(*e);
            }
        }
        *v = kept;
    }
    out
}

struct Current {
    input: f64,
    output: f64,
    cache_creation: Option<f64>,
    cache_read: Option<f64>,
}

#[derive(Deserialize)]
struct RawEntry {
    input_cost_per_token: Option<f64>,
    output_cost_per_token: Option<f64>,
    cache_creation_input_token_cost: Option<f64>,
    cache_read_input_token_cost: Option<f64>,
}

/// Current LiteLLM prices for tracked, un-prefixed keys.
fn parse_current(s: &str) -> anyhow::Result<BTreeMap<String, Current>> {
    let raw: HashMap<String, serde_json::Value> = serde_json::from_str(s)?;
    let lower: std::collections::HashSet<String> = raw.keys().map(|k| k.to_lowercase()).collect();
    let mut out = BTreeMap::new();
    for (key, val) in raw {
        let mut key = key.to_lowercase();
        let mut prefixed = false;
        if let Some(bare) = key.strip_prefix("gemini/") {
            key = bare.to_string();
            prefixed = true;
        } else if key.starts_with("gemini-") {
            // Gemini CLI uses Developer API rates. Bare LiteLLM Gemini keys
            // can refer to Vertex, so only accept explicitly Gemini entries.
            if lower.contains(&format!("gemini/{key}"))
                || val.get("litellm_provider").and_then(|p| p.as_str()) != Some("gemini")
            {
                continue;
            }
        }
        if let Some(bare) = key.strip_prefix(PROVIDER_PREFIX) {
            if lower.contains(bare) {
                continue; // bare key wins
            }
            key = bare.to_string();
            prefixed = true;
        }
        if key.contains('/') || !TRACKED_PREFIXES.iter().any(|p| key.starts_with(p)) {
            continue;
        }
        if key.starts_with("gpt-oss") {
            continue;
        }
        // bare keys served via Bedrock are excluded; openai.-prefixed keys are the allowed source
        if !prefixed
            && val
                .get("litellm_provider")
                .and_then(|p| p.as_str())
                .is_some_and(|p| p.starts_with("bedrock"))
        {
            continue;
        }
        let Ok(e) = serde_json::from_value::<RawEntry>(val) else {
            continue;
        };
        let (Some(input), Some(output)) = (e.input_cost_per_token, e.output_cost_per_token) else {
            continue;
        };
        out.insert(
            key,
            Current {
                input,
                output,
                cache_creation: e.cache_creation_input_token_cost,
                cache_read: e.cache_read_input_token_cost,
            },
        );
    }
    Ok(out)
}

/// New entries (at `now`) for models that are new or whose price changed vs. the latest entry.
fn diff_new_entries(
    current: &BTreeMap<String, Current>,
    table: &Models,
    now: DateTime<Utc>,
) -> Models {
    let mut out: Models = BTreeMap::new();
    for (k, c) in current {
        let e = Entry {
            from: now,
            input: c.input,
            output: c.output,
            cache_creation: c.cache_creation,
            cache_read: c.cache_read,
        };
        let latest = table.get(k).and_then(|v| v.last());
        if latest.map(|l| l.price()) != Some(e.price()) {
            out.insert(k.clone(), vec![e]);
        }
    }
    out
}

fn strip_tags(m: &str) -> &str {
    let mut m = m.trim_end();
    while m.ends_with(']') {
        match m.rfind('[') {
            Some(i) => m = m[..i].trim_end(),
            None => break,
        }
    }
    m
}

fn strip_date(m: &str) -> &str {
    let Some(i) = m.len().checked_sub(9).filter(|&i| i > 0) else {
        return m;
    };
    // `get` returns None when `i` is not a char boundary (non-ASCII input).
    match (m.get(..i), m.get(i..)) {
        (Some(head), Some(tail))
            if tail.starts_with('-') && tail[1..].bytes().all(|b| b.is_ascii_digit()) =>
        {
            head
        }
        _ => m,
    }
}

fn parse_bundled() -> anyhow::Result<History> {
    Ok(serde_json::from_str(BUNDLED)?)
}

/// `Ok(None)` when the local file does not exist.
fn read_local(dir: &Path) -> anyhow::Result<Option<History>> {
    let path = dir.join(LOCAL_FILE);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&std::fs::read_to_string(path)?)?))
}

/// Write to a temp file in `dir`, then rename over the target.
async fn persist_local(dir: &Path, h: &History) -> anyhow::Result<()> {
    let body = serde_json::to_vec(h)?;
    tokio::fs::create_dir_all(dir).await?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!("{LOCAL_FILE}.{}.{nanos}.tmp", std::process::id()));
    let res = async {
        tokio::fs::write(&tmp, body).await?;
        tokio::fs::rename(&tmp, dir.join(LOCAL_FILE)).await
    }
    .await;
    if let Err(e) = res {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e.into());
    }
    Ok(())
}

impl PriceTable {
    fn build(
        bundled: &History,
        local: Option<&History>,
        source: &str,
        updated_at: Option<DateTime<Utc>>,
        error: Option<String>,
    ) -> Self {
        let models = match local {
            Some(l) => merge(&bundled.models, &l.models),
            None => merge(&bundled.models, &BTreeMap::new()),
        };
        let model_count = models.len();
        Self {
            models,
            status: PricingStatus {
                source: source.into(),
                updated_at,
                model_count,
                error,
            },
        }
    }

    fn bundled_only(error: Option<String>) -> Self {
        match parse_bundled() {
            Ok(b) => Self::build(&b, None, "bundled", b.generated_at, error),
            Err(e) => Self {
                models: BTreeMap::new(),
                status: PricingStatus {
                    source: "bundled".into(),
                    updated_at: None,
                    model_count: 0,
                    error: Some(error.unwrap_or_else(|| format!("bundled prices invalid: {e}"))),
                },
            },
        }
    }

    pub fn load(cache_dir: &Path) -> Self {
        let Ok(bundled) = parse_bundled() else {
            return Self::bundled_only(None);
        };
        match read_local(cache_dir) {
            Ok(None) => Self::bundled_only(None),
            Ok(Some(l)) => {
                let at = l.generated_at.or(bundled.generated_at);
                Self::build(&bundled, Some(&l), "cache", at, None)
            }
            Err(e) => Self::bundled_only(Some(format!("local price history unreadable: {e}"))),
        }
    }

    /// Download current prices and record changes at `now`. A persist failure
    /// still returns the new table, with `status.error` set.
    pub async fn refresh(cache_dir: &Path) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()?;
        let body = client
            .get(PRICES_URL)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let current = parse_current(&body)?;
        if current.is_empty() {
            anyhow::bail!("no prices in response");
        }
        let bundled = parse_bundled()?;
        let dir = cache_dir.to_path_buf();
        // Corrupt local file is replaced by the fresh one.
        let local = tokio::task::spawn_blocking(move || read_local(&dir))
            .await?
            .ok()
            .flatten()
            .unwrap_or_default();
        let now = Utc::now();
        let merged = merge(&bundled.models, &local.models);
        let new = diff_new_entries(&current, &merged, now);
        let updated = History {
            source: "token-tracker local refresh".into(),
            generated_at: Some(now),
            head_commit: String::new(),
            models: merge(&local.models, &new),
        };
        let error = persist_local(cache_dir, &updated)
            .await
            .err()
            .map(|e| format!("could not save price history: {e}"));
        Ok(Self::build(
            &bundled,
            Some(&updated),
            "online",
            Some(now),
            error,
        ))
    }

    /// Price valid at `at`: latest entry with `from <= at`, first entry if `at`
    /// precedes all, latest entry if `at` is `None`.
    pub fn price_for(&self, model: &str, at: Option<DateTime<Utc>>) -> Option<ModelPrice> {
        let m = model.trim().to_lowercase();
        let m = m.strip_prefix("gemini/").unwrap_or(&m);
        let no_tags = strip_tags(m);
        let no_date = strip_date(no_tags);
        let get = |k: &str| self.models.get(k).filter(|v| !v.is_empty());
        let no_effort = ["-minimal", "-low", "-medium", "-high"]
            .iter()
            .find_map(|e| no_date.strip_suffix(e))
            .unwrap_or(no_date);
        let timeline = [m, no_tags, no_date, no_effort].into_iter().find_map(get)?;
        let entry = match at {
            None => timeline.last()?,
            Some(t) => {
                let n = timeline.partition_point(|e| e.from <= t);
                &timeline[n.saturating_sub(1)]
            }
        };
        Some(entry.price())
    }

    pub fn cost(&self, model: &str, usage: &TokenUsage, at: Option<DateTime<Utc>>) -> Option<f64> {
        let p = self.price_for(model, at)?;
        Some(
            usage.input as f64 * p.input
                + usage.output as f64 * p.output
                + usage.cache_creation as f64 * p.cache_creation
                + usage.cache_read as f64 * p.cache_read,
        )
    }

    pub fn status(&self) -> PricingStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn entry(from: &str, input: f64, output: f64) -> Entry {
        Entry {
            from: ts(from),
            input,
            output,
            cache_creation: None,
            cache_read: None,
        }
    }

    fn models(list: &[(&str, Vec<Entry>)]) -> Models {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn table(m: Models) -> PriceTable {
        let n = m.len();
        PriceTable {
            models: m,
            status: PricingStatus {
                source: "bundled".into(),
                updated_at: None,
                model_count: n,
                error: None,
            },
        }
    }

    fn timeline() -> PriceTable {
        table(models(&[(
            "claude-x-1",
            vec![
                entry("2025-01-01T00:00:00Z", 1.0, 2.0),
                entry("2025-06-01T00:00:00Z", 3.0, 4.0),
                entry("2025-09-01T00:00:00Z", 5.0, 6.0),
            ],
        )]))
    }

    #[test]
    fn timeline_selection() {
        let t = timeline();
        let at = |s: &str| t.price_for("claude-x-1", Some(ts(s))).unwrap().input;
        assert_eq!(at("2024-01-01T00:00:00Z"), 1.0);
        assert_eq!(at("2025-01-01T00:00:00Z"), 1.0);
        assert_eq!(at("2025-07-01T00:00:00Z"), 3.0);
        assert_eq!(at("2025-09-01T00:00:00Z"), 5.0);
        assert_eq!(at("2030-01-01T00:00:00Z"), 5.0);
        assert_eq!(t.price_for("claude-x-1", None).unwrap().input, 5.0);
    }

    #[test]
    fn missing_cache_defaults_to_input() {
        let p = timeline().price_for("claude-x-1", None).unwrap();
        assert_eq!(p.cache_creation, 5.0);
        assert_eq!(p.cache_read, 5.0);
    }

    #[test]
    fn lookup_chain() {
        let e = vec![entry("2025-01-01T00:00:00Z", 1.0, 1.0)];
        let t = table(models(&[
            ("claude-x-1", e.clone()),
            ("anthropic/claude-viaprefix", e.clone()),
            ("openai/gpt-viaprefix", e),
        ]));
        let inp = |m: &str| t.price_for(m, None).map(|p| p.input);
        assert_eq!(inp("claude-x-1"), Some(1.0));
        assert_eq!(inp("CLAUDE-X-1"), Some(1.0));
        // no namespace fallback
        assert_eq!(inp("claude-viaprefix"), None);
        assert_eq!(inp("gpt-viaprefix"), None);
        assert_eq!(inp("claude-x-1[1m]"), Some(1.0));
        assert_eq!(inp("claude-x-1-20250101"), Some(1.0));
        assert_eq!(inp("claude-x-1-20250101[1m]"), Some(1.0));
        // no prefix fallback
        assert_eq!(inp("claude-x-1-foo"), None);
        assert_eq!(inp("claude-opus-4-6-foo"), None);
        assert_eq!(inp("claude"), None);
    }

    #[test]
    fn effort_suffix_is_stripped_last() {
        let e = vec![entry("2025-01-01T00:00:00Z", 1.0, 1.0)];
        let e2 = vec![entry("2025-01-01T00:00:00Z", 2.0, 2.0)];
        let t = table(models(&[
            ("gemini-3.8-flash", e),
            ("gemini-3.8-pro-high", e2),
        ]));
        let inp = |m: &str| t.price_for(m, None).map(|p| p.input);
        assert_eq!(inp("gemini-3.8-flash-medium"), Some(1.0));
        assert_eq!(inp("gemini-3.8-flash-low"), Some(1.0));
        // exact key wins over stripping
        assert_eq!(inp("gemini-3.8-pro-high"), Some(2.0));
        assert_eq!(inp("gemini-3.8-flash-ultra"), None);
        assert_eq!(inp("unknown"), None);
    }

    #[test]
    fn openai_dot_prefix_is_stripped() {
        let json = r#"{
            "openai.gpt-6-luna": {"input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7,
                "cache_creation_input_token_cost": 1.25e-7, "cache_read_input_token_cost": 1e-8},
            "openai.gpt-5-codex": {"input_cost_per_token": 2e-7, "output_cost_per_token": 6e-7},
            "openai.gpt-5.1-codex": {"input_cost_per_token": 3e-7, "output_cost_per_token": 7e-7},
            "openai/gpt-viaprefix": {"input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7},
            "openai.gpt-oss-120b-1:0": {"input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7},
            "openai.gpt-oss-safeguard-20b": {"input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7},
            "gpt-oss-20b": {"input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7},
            "anthropic.claude-opus-4-6-v1": {"input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7},
            "amazon.nova-pro-v1:0": {"input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7},
            "claude-sonnet-4-5-20250929-v1:0": {"litellm_provider": "bedrock",
                "input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7}
        }"#;
        let cur = parse_current(json).unwrap();
        let now = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let t = table(diff_new_entries(&cur, &BTreeMap::new(), now));
        let inp = |m: &str| t.price_for(m, None).map(|p| p.input);
        assert_eq!(inp("gpt-6-luna"), Some(1e-7));
        assert_eq!(inp("gpt-5-codex"), Some(2e-7));
        assert_eq!(inp("gpt-5.1-codex"), Some(3e-7));
        assert_eq!(inp("gpt-viaprefix"), None);
        assert_eq!(inp("gpt-oss-120b-1:0"), None);
        assert_eq!(inp("gpt-oss-safeguard-20b"), None);
        assert_eq!(inp("gpt-oss-20b"), None);
        assert_eq!(inp("anthropic.claude-opus-4-6-v1"), None);
        assert_eq!(inp("claude-opus-4-6-v1"), None);
        assert_eq!(inp("amazon.nova-pro-v1:0"), None);
        assert_eq!(inp("claude-sonnet-4-5-20250929-v1:0"), None);
    }

    #[test]
    fn non_ascii_models_no_panic() {
        let t = timeline();
        for m in [
            "modèle-2025010",
            "modèle-20250101",
            "ž",
            "žžžžžžžžžž",
            "🦀",
            "🦀🦀🦀",
            "claude-x-1-é[1m",
            "é]",
            "[é]",
            "",
        ] {
            assert!(t.price_for(m, None).is_none(), "{m}");
        }
        assert_eq!(strip_date("modèle-2025010"), "modèle-2025010");
        assert_eq!(strip_date("abc"), "abc");
        assert_eq!(strip_date("-20250101"), "-20250101");
        assert_eq!(strip_date("a-20250101"), "a");
    }

    #[test]
    fn cost_math() {
        let t = table(models(&[(
            "claude-x-1",
            vec![Entry {
                from: ts("2025-01-01T00:00:00Z"),
                input: 0.000003,
                output: 0.000015,
                cache_creation: Some(0.00000375),
                cache_read: Some(0.0000003),
            }],
        )]));
        let u = TokenUsage {
            input: 1000,
            output: 100,
            cache_creation: 200,
            cache_read: 5000,
            reasoning: 50,
        };
        let c = t.cost("claude-x-1", &u, None).unwrap();
        let exp = 1000.0 * 0.000003 + 100.0 * 0.000015 + 200.0 * 0.00000375 + 5000.0 * 0.0000003;
        assert!((c - exp).abs() < 1e-12);
        assert!(t.cost("nope-model", &u, None).is_none());
    }

    #[test]
    fn merge_and_collapse() {
        let a = models(&[(
            "m",
            vec![
                entry("2025-01-01T00:00:00Z", 1.0, 1.0),
                entry("2025-03-01T00:00:00Z", 1.0, 1.0),
            ],
        )]);
        let b = models(&[
            (
                "M",
                vec![
                    entry("2025-05-01T00:00:00Z", 2.0, 2.0),
                    entry("2025-04-01T00:00:00Z", 2.0, 2.0),
                    entry("2025-06-01T00:00:00Z", 1.0, 1.0),
                ],
            ),
            ("n", vec![entry("2025-01-01T00:00:00Z", 9.0, 9.0)]),
        ]);
        let m = merge(&a, &b);
        let got: Vec<_> = m["m"].iter().map(|e| (e.from, e.input)).collect();
        assert_eq!(
            got,
            vec![
                (ts("2025-01-01T00:00:00Z"), 1.0),
                (ts("2025-04-01T00:00:00Z"), 2.0),
                (ts("2025-06-01T00:00:00Z"), 1.0),
            ]
        );
        assert_eq!(m["n"].len(), 1);
    }

    #[test]
    fn diff_logic() {
        let now = ts("2026-01-01T00:00:00Z");
        let cur = parse_current(
            r#"{
            "sample_spec": {"input_cost_per_token": 9, "output_cost_per_token": 9},
            "claude-x-1": {"input_cost_per_token": 5.0, "output_cost_per_token": 6.0},
            "claude-same": {"input_cost_per_token": 1.0, "output_cost_per_token": 1.0,
                "cache_read_input_token_cost": 1.0},
            "claude-new": {"input_cost_per_token": 7.0, "output_cost_per_token": 8.0},
            "claude-nooutput": {"input_cost_per_token": 7.0},
            "anthropic/claude-prefixed": {"input_cost_per_token": 7.0, "output_cost_per_token": 8.0},
            "Claude-Upper": {"input_cost_per_token": 2.0, "output_cost_per_token": 3.0},
            "untracked-model": {"input_cost_per_token": 7.0, "output_cost_per_token": 8.0}
        }"#,
        )
        .unwrap();
        assert_eq!(cur.len(), 4);
        let tbl = models(&[
            (
                "claude-x-1",
                vec![
                    entry("2025-01-01T00:00:00Z", 1.0, 2.0),
                    entry("2025-09-01T00:00:00Z", 5.0, 6.0),
                ],
            ),
            ("claude-same", vec![entry("2025-01-01T00:00:00Z", 1.0, 1.0)]),
        ]);
        let new = diff_new_entries(&cur, &tbl, now);
        let keys: Vec<_> = new.keys().cloned().collect();
        assert_eq!(keys, vec!["claude-new", "claude-upper"]);
        assert_eq!(new["claude-new"][0].from, now);
        // changed price
        let tbl2 = models(&[("claude-x-1", vec![entry("2025-01-01T00:00:00Z", 1.0, 2.0)])]);
        let new2 = diff_new_entries(&cur, &tbl2, now);
        assert_eq!(new2["claude-x-1"][0].input, 5.0);
    }

    #[test]
    fn bundled_history_parses() {
        let h = parse_bundled().unwrap();
        assert!(h.generated_at.is_some());
        assert!(h.models.contains_key("claude-opus-5-5"));
        assert!(h.models.contains_key("gpt-5"));
        for v in h.models.values() {
            assert!(v.windows(2).all(|w| w[0].from <= w[1].from));
        }
    }

    #[test]
    fn bundled_o3_history() {
        let t = PriceTable::load(Path::new("/nonexistent-cache-dir"));
        let u = TokenUsage {
            input: 1_000_000,
            output: 1_000_000,
            ..Default::default()
        };
        let c1 = t.cost("o3", &u, Some(ts("2025-05-01T00:00:00Z"))).unwrap();
        let c2 = t.cost("o3", &u, Some(ts("2025-07-01T00:00:00Z"))).unwrap();
        assert!((c1 - 50.0).abs() < 1e-6, "{c1}");
        assert!((c2 - 10.0).abs() < 1e-6, "{c2}");
    }

    #[test]
    fn load_local_vs_bundled() {
        let dir = tempfile::tempdir().unwrap();
        let st = PriceTable::load(dir.path()).status();
        assert_eq!(st.source, "bundled");
        assert!(st.updated_at.is_some() && st.error.is_none());
        let base_count = st.model_count;

        // legacy cache file is ignored
        std::fs::write(dir.path().join("litellm_prices.json"), "not json").unwrap();
        assert_eq!(PriceTable::load(dir.path()).status().source, "bundled");

        let when = ts("2026-02-03T04:05:06Z");
        let local = History {
            source: "t".into(),
            generated_at: Some(when),
            head_commit: String::new(),
            models: models(&[(
                "claude-local-1",
                vec![entry("2026-02-03T00:00:00Z", 1.0, 2.0)],
            )]),
        };
        std::fs::write(
            dir.path().join(LOCAL_FILE),
            serde_json::to_string(&local).unwrap(),
        )
        .unwrap();
        let t = PriceTable::load(dir.path());
        let st = t.status();
        assert_eq!(st.source, "cache");
        assert_eq!(st.updated_at, Some(when));
        assert_eq!(st.model_count, base_count + 1);
        assert!(t.price_for("claude-local-1", None).is_some());
        assert!(t.price_for("claude-opus-5-5", None).is_some());

        std::fs::write(dir.path().join(LOCAL_FILE), "not json").unwrap();
        let st = PriceTable::load(dir.path()).status();
        assert_eq!(st.source, "bundled");
        assert!(st.error.is_some());
        assert_eq!(st.model_count, base_count);
    }

    #[tokio::test]
    async fn persist_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("nested");
        let h = History {
            source: "t".into(),
            generated_at: Some(ts("2026-01-01T00:00:00Z")),
            head_commit: String::new(),
            models: models(&[("m", vec![entry("2026-01-01T00:00:00Z", 1.0, 2.0)])]),
        };
        persist_local(&sub, &h).await.unwrap();
        let back = read_local(&sub).unwrap().unwrap();
        assert_eq!(back.models["m"], h.models["m"]);
        persist_local(&sub, &h).await.unwrap();
        let names: Vec<_> = std::fs::read_dir(&sub)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![LOCAL_FILE.to_string()]);
    }
}
