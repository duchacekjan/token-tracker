use crate::model::TokenUsage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// USD per token.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
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

const BUNDLED: &str = include_str!("../resources/bundled_prices.json");
const PRICES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const CACHE_FILE: &str = "litellm_prices.json";

pub struct PriceTable {
    /// Keys lowercased.
    prices: HashMap<String, ModelPrice>,
    status: PricingStatus,
}

#[derive(Deserialize)]
struct RawEntry {
    input_cost_per_token: Option<f64>,
    output_cost_per_token: Option<f64>,
    cache_creation_input_token_cost: Option<f64>,
    cache_read_input_token_cost: Option<f64>,
}

fn parse_litellm(s: &str) -> anyhow::Result<HashMap<String, ModelPrice>> {
    let raw: HashMap<String, serde_json::Value> = serde_json::from_str(s)?;
    let mut out = HashMap::new();
    for (key, val) in raw {
        if key == "sample_spec" {
            continue;
        }
        let Ok(e) = serde_json::from_value::<RawEntry>(val) else {
            continue;
        };
        let (Some(input), Some(output)) = (e.input_cost_per_token, e.output_cost_per_token) else {
            continue;
        };
        out.insert(
            key.to_lowercase(),
            ModelPrice {
                input,
                output,
                cache_creation: e.cache_creation_input_token_cost.unwrap_or(input),
                cache_read: e.cache_read_input_token_cost.unwrap_or(input),
            },
        );
    }
    Ok(out)
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
    if m.len() > 9 {
        let (head, tail) = m.split_at(m.len() - 9);
        if tail.starts_with('-') && tail[1..].bytes().all(|b| b.is_ascii_digit()) {
            return head;
        }
    }
    m
}

impl PriceTable {
    #[cfg(test)]
    pub(crate) fn from_litellm_json(s: &str) -> anyhow::Result<HashMap<String, ModelPrice>> {
        parse_litellm(s)
    }

    fn with_source(
        prices: HashMap<String, ModelPrice>,
        source: &str,
        updated_at: Option<DateTime<Utc>>,
        error: Option<String>,
    ) -> Self {
        let model_count = prices.len();
        Self {
            prices,
            status: PricingStatus {
                source: source.into(),
                updated_at,
                model_count,
                error,
            },
        }
    }

    fn bundled(error: Option<String>) -> Self {
        match parse_litellm(BUNDLED) {
            Ok(p) => Self::with_source(p, "bundled", None, error),
            Err(e) => Self::with_source(
                HashMap::new(),
                "bundled",
                None,
                Some(error.unwrap_or_else(|| format!("bundled prices invalid: {e}"))),
            ),
        }
    }

    pub fn load(cache_dir: &Path) -> Self {
        let path = cache_dir.join(CACHE_FILE);
        if !path.exists() {
            return Self::bundled(None);
        }
        let res = (|| -> anyhow::Result<Self> {
            let body = std::fs::read_to_string(&path)?;
            let prices = parse_litellm(&body)?;
            let mtime = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .map(DateTime::<Utc>::from);
            Ok(Self::with_source(prices, "cache", mtime, None))
        })();
        match res {
            Ok(t) => t,
            Err(e) => Self::bundled(Some(format!("cache unreadable: {e}"))),
        }
    }

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
        let prices = parse_litellm(&body)?;
        if prices.is_empty() {
            anyhow::bail!("no prices in response");
        }
        std::fs::create_dir_all(cache_dir)?;
        std::fs::write(cache_dir.join(CACHE_FILE), &body)?;
        Ok(Self::with_source(prices, "online", Some(Utc::now()), None))
    }

    pub fn price_for(&self, model: &str) -> Option<ModelPrice> {
        let m = model.trim().to_lowercase();
        let no_tags = strip_tags(&m);
        let no_date = strip_date(no_tags);
        let get = |k: &str| self.prices.get(k).copied();
        for base in [m.as_str(), no_tags, no_date] {
            if let Some(p) = get(base)
                .or_else(|| get(&format!("anthropic/{base}")))
                .or_else(|| get(&format!("openai/{base}")))
            {
                return Some(p);
            }
        }
        self.prices
            .iter()
            .filter(|(k, _)| k.len() >= 5 && no_date.starts_with(k.as_str()))
            .max_by_key(|(k, _)| k.len())
            .map(|(_, p)| *p)
    }

    pub fn cost(&self, model: &str, usage: &TokenUsage) -> Option<f64> {
        let p = self.price_for(model)?;
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

    const FIX: &str = r#"{
        "sample_spec": {"input_cost_per_token": 9, "output_cost_per_token": 9},
        "claude-x-1": {"input_cost_per_token": 0.000003, "output_cost_per_token": 0.000015,
            "cache_creation_input_token_cost": 0.00000375, "cache_read_input_token_cost": 0.0000003},
        "gpt-test": {"input_cost_per_token": 0.000001, "output_cost_per_token": 0.000004},
        "anthropic/claude-viaprefix": {"input_cost_per_token": 2.0, "output_cost_per_token": 3.0},
        "openai/gpt-viaprefix": {"input_cost_per_token": 4.0, "output_cost_per_token": 5.0},
        "gpt-short": {"input_cost_per_token": 1.0, "output_cost_per_token": 1.0},
        "gpt-short-longer": {"input_cost_per_token": 7.0, "output_cost_per_token": 7.0},
        "no-output": {"input_cost_per_token": 1.0}
    }"#;

    fn table(json: &str) -> PriceTable {
        PriceTable::with_source(parse_litellm(json).unwrap(), "bundled", None, None)
    }

    #[test]
    fn parses_and_skips() {
        let p = PriceTable::from_litellm_json(FIX).unwrap();
        assert!(!p.contains_key("sample_spec"));
        assert!(!p.contains_key("no-output"));
        assert_eq!(p.len(), 6);
        assert_eq!(p["claude-x-1"].cache_read, 0.0000003);
    }

    #[test]
    fn missing_cache_defaults_to_input() {
        let p = table(FIX).price_for("gpt-test").unwrap();
        assert_eq!(p.cache_creation, 0.000001);
        assert_eq!(p.cache_read, 0.000001);
    }

    #[test]
    fn lookup_fallbacks() {
        let t = table(FIX);
        assert_eq!(t.price_for("claude-x-1").unwrap().input, 0.000003);
        assert_eq!(t.price_for("CLAUDE-X-1").unwrap().input, 0.000003);
        assert_eq!(t.price_for("claude-viaprefix").unwrap().input, 2.0);
        assert_eq!(t.price_for("gpt-viaprefix").unwrap().input, 4.0);
        assert_eq!(t.price_for("claude-x-1[1m]").unwrap().input, 0.000003);
        assert_eq!(t.price_for("claude-x-1-20250101").unwrap().input, 0.000003);
        assert_eq!(t.price_for("claude-x-1-20250101[1m]").unwrap().input, 0.000003);
        // prefix: longest key wins
        assert_eq!(t.price_for("gpt-short-foo").unwrap().input, 1.0);
        assert_eq!(t.price_for("gpt-short-longer-foo").unwrap().input, 7.0);
        assert!(t.price_for("unknown-model").is_none());
        assert!(t.price_for("gpt").is_none());
    }

    #[test]
    fn cost_math() {
        let t = table(FIX);
        let u = TokenUsage {
            input: 1000,
            output: 100,
            cache_creation: 200,
            cache_read: 5000,
            reasoning: 50,
        };
        let c = t.cost("claude-x-1", &u).unwrap();
        let exp = 1000.0 * 0.000003 + 100.0 * 0.000015 + 200.0 * 0.00000375 + 5000.0 * 0.0000003;
        assert!((c - exp).abs() < 1e-12);
        assert!(t.cost("nope-model", &u).is_none());
    }

    #[test]
    fn load_cache_vs_bundled() {
        let dir = tempfile::tempdir().unwrap();
        let b = PriceTable::load(dir.path());
        let st = b.status();
        assert_eq!(st.source, "bundled");
        assert!(st.updated_at.is_none() && st.error.is_none());

        std::fs::write(dir.path().join(CACHE_FILE), FIX).unwrap();
        let c = PriceTable::load(dir.path());
        let st = c.status();
        assert_eq!(st.source, "cache");
        assert!(st.updated_at.is_some());
        assert_eq!(st.model_count, 6);

        std::fs::write(dir.path().join(CACHE_FILE), "not json").unwrap();
        let st = PriceTable::load(dir.path()).status();
        assert_eq!(st.source, "bundled");
        assert!(st.error.is_some());
    }

    #[test]
    fn bundled_parses() {
        let p = parse_litellm(BUNDLED).unwrap();
        assert!(p.keys().any(|k| k.starts_with("claude-")));
        assert!(p.keys().any(|k| k.starts_with("gpt-5")));
    }
}
