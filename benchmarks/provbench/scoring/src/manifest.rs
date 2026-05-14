//! Read-side `SampleManifest` for the shared scorer.
//!
//! Baseline owns the writer-side `SampleManifest` (with corpus loading,
//! diff/fact joins, content-hashing, …). The scorer only needs to load
//! `manifest.json` and read a small set of provenance fields, so the
//! scoring crate carries this slimmer reader-side mirror. Both shapes
//! must serialise/deserialise the same JSON; baseline's writer is the
//! source of truth for the byte layout.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Per-stratum sample targets. `usize::MAX` is the sentinel meaning
/// "take the entire stratum".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerStratumTargets {
    pub valid: usize,
    pub stale_changed: usize,
    pub stale_deleted: usize,
    pub stale_renamed: usize,
    pub needs_revalidation: usize,
}

/// One sampled row, as recorded in `manifest.json`. Read-only mirror —
/// the writer-side definition lives in `provbench-baseline`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampledRow {
    pub fact_id: String,
    pub commit_sha: String,
    pub ground_truth: String,
    /// Coalesced stratum tag (`Valid`, `StaleChanged`, …). Kept as a raw
    /// `serde_json::Value` because the scorer doesn't dispatch on it.
    #[serde(default)]
    pub stratum: serde_json::Value,
}

/// Read-side mirror of `SampleManifest`. Field set is the union of what
/// the baseline writer emits and what `report.rs` reads by name.
/// Unknown writer-side fields are tolerated via serde's default
/// behaviour (no `deny_unknown_fields`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleManifest {
    pub seed: u64,
    #[serde(default)]
    pub corpus_path: PathBuf,
    #[serde(default)]
    pub facts_path: PathBuf,
    #[serde(default)]
    pub diffs_dir: PathBuf,
    pub labeler_git_sha: String,
    pub spec_freeze_hash: String,
    #[serde(default)]
    pub baseline_crate_head_sha: String,
    #[serde(default)]
    pub per_stratum_targets: Option<PerStratumTargets>,
    #[serde(default)]
    pub selected_count: usize,
    #[serde(default)]
    pub excluded_count_by_reason: BTreeMap<String, usize>,
    #[serde(default)]
    pub estimated_worst_case_usd: f64,
    #[serde(default)]
    pub rows: Vec<SampledRow>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub content_hash: String,
}
