//! `score` subcommand — reads a completed run directory, computes §7.1 /
//! §7.2-applicable / §9.2 metrics, writes `metrics.json` atomically.

use crate::constants::{MODEL_ID, MODEL_SNAPSHOT_DATE};
use crate::manifest::SampleManifest;
use crate::metrics;
use crate::runner::{PredictionRow, RunResult};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

/// Labeler-corpus population counts per stratum, used to build
/// Horvitz-Thompson weights for §9.2 overall agreement. Counts are
/// pinned from the ripgrep corpus run referenced by SPEC §9.2 — they
/// describe the universe the sample is drawn from, not the sample
/// itself.
pub const POP_VALID: u64 = 2_045_512;
pub const POP_STALE_CHANGED: u64 = 210_516;
pub const POP_STALE_DELETED: u64 = 187_727;
pub const POP_STALE_RENAMED: u64 = 1_232;
pub const POP_NEEDS_REVAL: u64 = 27_916;

/// Total stale population (sum of the three Stale* subtypes).
pub const POP_STALE_TOTAL: u64 = POP_STALE_CHANGED + POP_STALE_DELETED + POP_STALE_RENAMED;

/// Compose population weights for the 3-class coalesced axis from the
/// labeler corpus totals.
///
/// The weights are normalised to sum to 1.0 (so they're directly usable
/// as HT renormalisation factors). The manifest argument is currently
/// unused but kept on the signature for forward-compat — a future
/// labeler revision may attach corpus counts to the manifest, at which
/// point this function will prefer those over the baked-in constants.
pub fn compute_population_weights(_manifest: &SampleManifest) -> HashMap<String, f64> {
    let total = (POP_VALID + POP_STALE_TOTAL + POP_NEEDS_REVAL) as f64;
    let mut w = HashMap::new();
    w.insert("valid".to_string(), POP_VALID as f64 / total);
    w.insert("stale".to_string(), POP_STALE_TOTAL as f64 / total);
    w.insert(
        "needs_revalidation".to_string(),
        POP_NEEDS_REVAL as f64 / total,
    );
    w
}

/// Count rows per raw stratum tag (`Valid`, `StaleSourceChanged`, …).
pub fn count_per_stratum(predictions: &[PredictionRow]) -> HashMap<String, u64> {
    let mut out: HashMap<String, u64> = HashMap::new();
    for row in predictions {
        *out.entry(row.ground_truth.clone()).or_insert(0) += 1;
    }
    out
}

/// Read `<run_dir>/manifest.json`.
fn load_manifest(run_dir: &Path) -> Result<SampleManifest> {
    let p = run_dir.join("manifest.json");
    let bytes = std::fs::read(&p).with_context(|| format!("read {}", p.display()))?;
    let m: SampleManifest =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", p.display()))?;
    Ok(m)
}

/// Read `<run_dir>/predictions.jsonl`.
pub fn load_predictions(run_dir: &Path) -> Result<Vec<PredictionRow>> {
    let p = run_dir.join("predictions.jsonl");
    let text = std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    let mut out = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row: PredictionRow = serde_json::from_str(line)
            .with_context(|| format!("parse {}:{}", p.display(), lineno + 1))?;
        out.push(row);
    }
    Ok(out)
}

/// Read `<run_dir>/run_meta.json` if it exists. Returns `None` for
/// dry-run / unit-test fixtures that skipped writing it.
fn load_run_meta(run_dir: &Path) -> Result<Option<RunResult>> {
    let p = run_dir.join("run_meta.json");
    if !p.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&p).with_context(|| format!("read {}", p.display()))?;
    let r: RunResult =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", p.display()))?;
    Ok(Some(r))
}

/// Score a completed run directory, writing `metrics.json` atomically.
pub fn score_run(run_dir: &Path) -> Result<()> {
    let manifest = load_manifest(run_dir)?;
    let predictions = load_predictions(run_dir)?;
    let run_meta = load_run_meta(run_dir)?;
    let pop_weights = compute_population_weights(&manifest);
    let per_stratum_sizes = count_per_stratum(&predictions);

    let three = metrics::three_way(&predictions, &pop_weights);
    let agreement = metrics::llm_validator_agreement(&predictions, &pop_weights);
    let latency = metrics::latency(&predictions);
    let total_cost_usd = run_meta.as_ref().map(|r| r.total_cost_usd).unwrap_or(0.0);
    let cost = metrics::cost_per_correct_invalidation_from_total(&predictions, total_cost_usd);

    let json = serde_json::json!({
        "spec_freeze_hash": manifest.spec_freeze_hash,
        "labeler_git_sha": manifest.labeler_git_sha,
        "model_id": MODEL_ID,
        "model_snapshot_date": MODEL_SNAPSHOT_DATE,
        "sample_seed": format!("{:#018x}", manifest.seed),
        "coverage": "subset",
        "per_stratum_sizes": per_stratum_sizes,
        "population_weights": pop_weights,
        "section_7_1": {
            "stale_detection": {
                "precision": three.stale_detection.precision,
                "recall": three.stale_detection.recall,
                "f1": three.stale_detection.f1,
                "wilson_lower_95": three.stale_detection.wilson_lower_95,
            },
            "valid_retention_accuracy": {
                "point": three.valid_retention_accuracy.point,
                "wilson_lower_95": three.valid_retention_accuracy.wilson_lower_95,
            },
            "needs_revalidation_routing_accuracy": {
                "point": three.needs_revalidation_routing_accuracy.point,
                "wilson_lower_95": three.needs_revalidation_routing_accuracy.wilson_lower_95,
            },
        },
        "section_7_2_applicable": {
            "latency_p50_ms": latency.p50_ms,
            "latency_p95_ms": latency.p95_ms,
            "cost_per_correct_invalidation": {
                "tokens": cost.tokens,
                "usd": cost.usd,
            },
        },
        "llm_validator_agreement": {
            "overall": {
                "point": agreement.overall.point,
                "ht_se": agreement.overall.ht_se,
            },
            "per_class": agreement.per_class,
            "confusion_matrix_3x3": agreement.confusion_matrix_3x3,
            "cohen_kappa": {
                "point_estimate": agreement.cohen_kappa.point_estimate,
                "ci_95_lower": agreement.cohen_kappa.ci_95_lower,
                "ci_95_upper": agreement.cohen_kappa.ci_95_upper,
                "n_bootstrap": agreement.cohen_kappa.n_bootstrap,
            },
            "per_stale_subtype": agreement.per_stale_subtype,
        }
    });

    let out_path = run_dir.join("metrics.json");
    let tmp = run_dir.join(format!(".metrics.tmp.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&json)?)
        .with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &out_path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), out_path.display()))?;
    Ok(())
}
