//! Side-by-side metrics builder (LLM baseline column + candidate column).
//!
//! Loads the baseline run's already-scored `metrics.json`, scores the
//! candidate run's `predictions.jsonl` against the same SPEC §7.1 axes,
//! and emits a single JSON document with both columns, per-rule confusion
//! (joined from `rule_traces.jsonl`), and SPEC §8 threshold flags.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::PredictionRow;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Compare {
    pub llm_baseline: Value,
    pub candidate: Value,
    pub candidate_name: String,
    pub deltas: BTreeMap<String, f64>,
    pub thresholds: BTreeMap<String, bool>,
    pub per_rule_confusion: BTreeMap<String, BTreeMap<String, u64>>,
}

pub fn run(baseline_run: &Path, candidate_run: &Path, candidate_name: &str) -> Result<Value> {
    // 1) Read the baseline run's pre-scored metrics.json.
    let baseline_metrics: Value = {
        let path = baseline_run.join("metrics.json");
        let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_slice(&bytes)?
    };

    // 2) Score the candidate predictions.jsonl directly.
    let candidate_metrics: Value = score_candidate(candidate_run)?;

    // 3) Build deltas and SPEC §8 threshold flags.
    let stale_recall_wlb = candidate_metrics["section_7_1"]["stale_detection"]["wilson_lower_95"]
        .as_f64()
        .unwrap_or(0.0);
    let valid_acc_wlb = candidate_metrics["section_7_1"]["valid_retention_accuracy"]
        ["wilson_lower_95"]
        .as_f64()
        .unwrap_or(0.0);
    let p50 = candidate_metrics["section_7_2_applicable"]["latency_p50_ms"]
        .as_u64()
        .unwrap_or(u64::MAX);
    let baseline_p50 = baseline_metrics["section_7_2_applicable"]["latency_p50_ms"]
        .as_u64()
        .unwrap_or(u64::MAX);

    let mut deltas: BTreeMap<String, f64> = BTreeMap::new();
    deltas.insert(
        "latency_p50_ms_speedup".into(),
        (baseline_p50 as f64) / (p50.max(1) as f64),
    );
    let mut thresholds: BTreeMap<String, bool> = BTreeMap::new();
    thresholds.insert(
        "section_8_3_valid_retention_ge_0_95".into(),
        valid_acc_wlb >= 0.95,
    );
    thresholds.insert("section_8_4_latency_p50_le_727_ms".into(), p50 <= 727);
    thresholds.insert(
        "section_8_5_stale_recall_wlb_ge_0_30".into(),
        stale_recall_wlb >= 0.30,
    );

    // 4) Per-rule confusion (joined from candidate_run/rule_traces.jsonl).
    let per_rule_confusion = load_per_rule_confusion(candidate_run)?;

    Ok(json!({
        "llm_baseline": baseline_metrics,
        candidate_name: candidate_metrics,
        "deltas": deltas,
        "thresholds": thresholds,
        "per_rule_confusion": per_rule_confusion,
    }))
}

fn score_candidate(candidate_run: &Path) -> Result<Value> {
    let preds_path = candidate_run.join("predictions.jsonl");
    let text = fs::read_to_string(&preds_path)
        .with_context(|| format!("reading {}", preds_path.display()))?;
    let mut rows: Vec<PredictionRow> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line)?);
    }

    let total = rows.len() as u64;
    let mut stale_tp = 0u64;
    let mut stale_fn_ = 0u64;
    let mut valid_correct = 0u64;
    let mut valid_total = 0u64;
    for r in &rows {
        let gt = r.ground_truth.to_lowercase();
        let pr = r.prediction.to_lowercase();
        if gt.starts_with("stale") {
            if pr == "stale" {
                stale_tp += 1
            } else {
                stale_fn_ += 1
            }
        } else if gt == "valid" {
            valid_total += 1;
            if pr == "valid" {
                valid_correct += 1
            }
        }
    }
    let stale_recall = if (stale_tp + stale_fn_) == 0 {
        0.0
    } else {
        stale_tp as f64 / (stale_tp + stale_fn_) as f64
    };
    let valid_acc = if valid_total == 0 {
        0.0
    } else {
        valid_correct as f64 / valid_total as f64
    };
    let stale_wlb = crate::metrics::wilson_lower_95(stale_tp, stale_tp + stale_fn_);
    let valid_wlb = crate::metrics::wilson_lower_95(valid_correct, valid_total);

    // Latency p50 — use the same wall_ms convention as the baseline
    // scorer (per-row wall_ms; phase1 records per-row classification cost).
    let mut walls: Vec<u64> = rows.iter().map(|r| r.wall_ms).collect();
    walls.sort();
    let p50 = if walls.is_empty() {
        0
    } else {
        walls[walls.len() / 2]
    };

    Ok(json!({
        "row_count": total,
        "section_7_1": {
            "stale_detection": {
                "recall": stale_recall,
                "wilson_lower_95": stale_wlb,
            },
            "valid_retention_accuracy": {
                "point": valid_acc,
                "wilson_lower_95": valid_wlb,
            },
        },
        "section_7_2_applicable": { "latency_p50_ms": p50 },
    }))
}

fn load_per_rule_confusion(
    candidate_run: &Path,
) -> Result<BTreeMap<String, BTreeMap<String, u64>>> {
    let traces = candidate_run.join("rule_traces.jsonl");
    let preds = candidate_run.join("predictions.jsonl");
    let mut row_to_rule: BTreeMap<i64, String> = BTreeMap::new();
    if let Ok(text) = fs::read_to_string(&traces) {
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(line)?;
            let row_index = v["row_index"].as_i64().unwrap_or(-1);
            let rule_id = v["rule_id"].as_str().unwrap_or("?").to_string();
            row_to_rule.insert(row_index, rule_id);
        }
    }
    let mut out: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let text = fs::read_to_string(&preds)?;
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let r: PredictionRow = serde_json::from_str(line)?;
        let rule = row_to_rule
            .get(&(i as i64))
            .cloned()
            .unwrap_or_else(|| "?".to_string());
        let bucket = out.entry(rule).or_default();
        let key = format!(
            "{}__{}",
            r.ground_truth.to_lowercase(),
            r.prediction.to_lowercase()
        );
        *bucket.entry(key).or_insert(0) += 1;
    }
    Ok(out)
}

/// Bench helper — returns the value and discards timing (no tracing dep
/// in this crate). Kept so callers that want to wrap a block in a timer
/// can be added later without changing the public API.
pub fn _timed<F: FnOnce() -> R, R>(_label: &str, f: F) -> R {
    let _s = Instant::now();
    f()
}
