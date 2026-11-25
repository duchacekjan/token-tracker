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

pub struct PriceTable {
    prices: HashMap<String, ModelPrice>,
    status: PricingStatus,
}

impl PriceTable {
    pub fn load(_cache_dir: &Path) -> Self {
        Self {
            prices: HashMap::new(),
            status: PricingStatus {
                source: "bundled".into(),
                updated_at: None,
                model_count: 0,
                error: None,
            },
        }
    }

    pub async fn refresh(_cache_dir: &Path) -> anyhow::Result<Self> {
        anyhow::bail!("not implemented")
    }

    pub fn price_for(&self, _model: &str) -> Option<ModelPrice> {
        let _ = &self.prices;
        None
    }

    pub fn cost(&self, _model: &str, _usage: &TokenUsage) -> Option<f64> {
        None
    }

    pub fn status(&self) -> PricingStatus {
        self.status.clone()
    }
}
