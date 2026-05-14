use serde::{Deserialize, Serialize};

/// Per-row checkpoint persisted to `predictions.jsonl`.
///
/// One row per line. JSON field order is fixed by serde derive order;
/// existing rows are never rewritten so determinism is preserved across
/// resumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionRow {
    pub fact_id: String,
    pub commit_sha: String,
    pub batch_id: String,
    pub ground_truth: String,
    pub prediction: String,
    pub request_id: String,
    pub wall_ms: u64,
}

/// Read-side mirror of `run_meta.json` — only the fields the scorer
/// consumes. Optional fields default so partial/legacy run-metas still
/// deserialise.
#[derive(Debug, Clone, Deserialize)]
pub struct RunResult {
    #[serde(default)]
    pub total_cost_usd: f64,
    #[serde(default)]
    pub total_tokens: u64,
}
