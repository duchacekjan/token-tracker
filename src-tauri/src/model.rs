use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Claude,
    Codex,
    Gemini,
}

/// `input` = NON-cached input tokens. `reasoning` is informational
/// (already included in `output` for Codex).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub reasoning: u64,
}

impl TokenUsage {
    pub fn add(&mut self, other: &TokenUsage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_creation += other.cache_creation;
        self.cache_read += other.cache_read;
        self.reasoning += other.reasoning;
    }

    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_creation + self.cache_read
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTurn {
    pub timestamp: Option<DateTime<Utc>>,
    pub model: String,
    pub usage: TokenUsage,
    pub dedup_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedSession {
    pub id: String,
    pub source: Source,
    pub project: Option<String>,
    pub file_path: String,
    pub turns: Vec<ParsedTurn>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub is_subagent: bool,
    /// Subagent `meta.json` description.
    #[serde(default)]
    pub agent_label: Option<String>,
    /// Subagent `meta.json` agentType.
    #[serde(default)]
    pub agent_type: Option<String>,
    /// Always false from parsers; a DB layer sets it later.
    #[serde(default)]
    pub log_deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub timestamp: Option<DateTime<Utc>>,
    pub model: String,
    pub usage: TokenUsage,
    pub cost_usd: Option<f64>,
    /// Index into `SessionSummary::transcripts`.
    #[serde(default)]
    pub transcript: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSummary {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub file_path: String,
    pub agent_id: Option<String>,
    pub agent_label: Option<String>,
    pub agent_type: Option<String>,
    pub is_subagent: bool,
    pub log_deleted: bool,
    pub models: Vec<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub usage: TokenUsage,
    pub cost_usd: Option<f64>,
    pub unpriced_models: Vec<String>,
    pub turn_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub source: Source,
    pub project: Option<String>,
    pub file_path: String,
    pub models: Vec<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub usage: TokenUsage,
    pub cost_usd: Option<f64>,
    pub unpriced_models: Vec<String>,
    pub turn_count: usize,
    pub subagent_count: usize,
    /// True only if ALL transcripts' logs are deleted.
    pub log_deleted: bool,
    /// Main first, then subagents by started_at.
    pub transcripts: Vec<TranscriptSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub turns: Vec<Turn>,
}
