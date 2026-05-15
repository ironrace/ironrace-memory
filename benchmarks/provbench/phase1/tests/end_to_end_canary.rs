use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

// Gate 3 (false-Valid safety bound from the dropped Field length guard):
// v1.2a must not increase the count of `stalesourcechanged__valid` for
// kind=Field by more than +20 vs the v1.1 pilot. The actual v1.1 Field
// count is loaded at test runtime from the v1.1 predictions to keep this
// resilient to changes in the v1.1 artifact (single source of truth).
const V1_2A_FIELD_FALSE_VALID_SLACK: usize = 20;

/// SPEC §8 gate:
///   §8 #3 valid_retention_accuracy.wilson_lower_95 >= 0.95
///   §8 #4 latency_p50_ms <= 727
///   §8 #5 stale_detection.recall.wilson_lower_95 >= 0.30
///
/// This test runs the full phase1 pipeline + provbench-score compare and
/// asserts all three thresholds clear on the Phase 0c canary.
#[test]
#[ignore = "requires benchmarks/provbench/work/ripgrep checkout; run with --ignored"]
fn spec_section_8_thresholds_clear_on_canary() {
    let provbench = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let workrepo = provbench.join("work/ripgrep");
    assert!(
        workrepo.exists(),
        "needs work/ripgrep checkout for end-to-end run"
    );

    let phase1_bin = env!("CARGO_BIN_EXE_provbench-phase1");
    let score_bin = ensure_scoring_binary_built();

    let out = TempDir::new().unwrap();
    let out_p = out.path().to_path_buf();

    let status = Command::new(phase1_bin)
        .args([
            "score",
            "--repo",
            workrepo.to_str().unwrap(),
            "--t0",
            "af6b6c543b224d348a8876f0c06245d9ea7929c5",
            "--facts",
            provbench
                .join("facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl")
                .to_str()
                .unwrap(),
            "--diffs-dir",
            provbench
                .join("facts/ripgrep-af6b6c54-c2d3b7b.diffs")
                .to_str()
                .unwrap(),
            "--baseline-run",
            provbench
                .join("results/phase0c/2026-05-13-canary")
                .to_str()
                .unwrap(),
            "--out",
            out_p.to_str().unwrap(),
            "--rule-set-version",
            "v1.2",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "phase1 score failed");

    let status = Command::new(&score_bin)
        .args([
            "compare",
            "--baseline-run",
            provbench
                .join("results/phase0c/2026-05-13-canary")
                .to_str()
                .unwrap(),
            "--candidate-run",
            out_p.to_str().unwrap(),
            "--candidate-name",
            "phase1_rules",
            "--out",
            out_p.join("metrics.json").to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "provbench-score compare failed");

    let metrics: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out_p.join("metrics.json")).unwrap()).unwrap();

    let stale_recall_wlb = metrics["phase1_rules"]["section_7_1"]["stale_detection"]
        ["wilson_lower_95"]
        .as_f64()
        .unwrap();
    let valid_acc_wlb = metrics["phase1_rules"]["section_7_1"]["valid_retention_accuracy"]
        ["wilson_lower_95"]
        .as_f64()
        .unwrap();
    let p50 = metrics["phase1_rules"]["section_7_2_applicable"]["latency_p50_ms"]
        .as_u64()
        .unwrap();
    let p95 = metrics["phase1_rules"]["section_7_2_applicable"]["latency_p95_ms"]
        .as_u64()
        .unwrap();
    let stale_precision = metrics["phase1_rules"]["section_7_1"]["stale_detection"]["precision"]
        .as_f64()
        .unwrap();
    let stale_f1 = metrics["phase1_rules"]["section_7_1"]["stale_detection"]["f1"]
        .as_f64()
        .unwrap();
    let needs_reval_point = metrics["phase1_rules"]["section_7_1"]
        ["needs_revalidation_routing_accuracy"]["point"]
        .as_f64()
        .unwrap();

    assert!(
        stale_recall_wlb >= 0.30,
        "§8 #5 stale recall WLB {:.4} < 0.30",
        stale_recall_wlb
    );
    assert!(
        valid_acc_wlb >= 0.95,
        "§8 #3 valid retention WLB {:.4} < 0.95",
        valid_acc_wlb
    );
    assert!(p50 <= 727, "§8 #4 latency p50 {} ms > 727", p50);
    assert!(
        p95 >= p50,
        "candidate p95 {} ms must be >= p50 {}",
        p95,
        p50
    );

    // Gate 2 (no regression vs v1.1 pilot).
    let v1_1_metrics = provbench.join("results/phase1/2026-05-15-canary/metrics.json");
    let v1_1_baseline =
        load_v1_1_gate2_baseline(&v1_1_metrics).expect("load v1.1 pilot Gate 2 baseline");
    assert!(
        stale_recall_wlb >= v1_1_baseline.stale_recall_wlb,
        "Gate 2 regression: stale recall WLB {:.4} < v1.1 pilot {:.4}",
        stale_recall_wlb,
        v1_1_baseline.stale_recall_wlb
    );
    assert!(
        valid_acc_wlb >= v1_1_baseline.valid_retention_wlb,
        "Gate 2 regression: valid retention WLB {:.4} < v1.1 pilot {:.4}",
        valid_acc_wlb,
        v1_1_baseline.valid_retention_wlb
    );
    assert!(
        p50 <= v1_1_baseline.latency_p50_ms + 5,
        "Gate 2 regression: latency p50 {} ms > v1.1 pilot {} ms + 5 ms slack",
        p50,
        v1_1_baseline.latency_p50_ms
    );

    // Gate 3 (false-Valid safety bound from the dropped Field length
    // guard): v1.2a count of stalesourcechanged__valid for kind=Field
    // must not exceed the v1.1 pilot count by more than the slack.
    let v1_1_predictions = provbench.join("results/phase1/2026-05-15-canary/predictions.jsonl");
    let v1_2_predictions = out_p.join("predictions.jsonl");
    let facts_path = provbench.join("facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl");
    assert!(
        v1_1_predictions.exists(),
        "v1.1 pilot predictions.jsonl not found at {} — Gate 3 cannot compute baseline",
        v1_1_predictions.display()
    );
    assert!(
        v1_2_predictions.exists(),
        "v1.2 candidate predictions.jsonl not found at {} — phase1 score did not emit it",
        v1_2_predictions.display()
    );
    let n_v1_1_field = count_stalesourcechanged_valid_field(&v1_1_predictions, &facts_path)
        .expect("count v1.1 Field false-Valid");
    let n_v1_2_field = count_stalesourcechanged_valid_field(&v1_2_predictions, &facts_path)
        .expect("count v1.2 Field false-Valid");
    assert!(
        n_v1_2_field <= n_v1_1_field + V1_2A_FIELD_FALSE_VALID_SLACK,
        "Gate 3 violation: v1.2a Field false-Valid count {} > v1.1 pilot {} + slack {}",
        n_v1_2_field,
        n_v1_1_field,
        V1_2A_FIELD_FALSE_VALID_SLACK
    );

    assert!(
        stale_precision > 0.0 && stale_f1 > 0.0,
        "candidate column must include full stale-detection precision/F1"
    );
    assert_eq!(
        needs_reval_point, 0.0,
        "canary emits no needs_revalidation predictions, but the metric must be present"
    );
    for key in [
        "stale_recall_point_delta",
        "stale_precision_point_delta",
        "valid_retention_wilson_lower_95_delta",
        "needs_revalidation_routing_wilson_lower_95_delta",
        "latency_p50_ratio_baseline_per_commit_to_candidate_per_row",
        "cost_per_correct_invalidation_usd_delta",
        "cost_per_correct_invalidation_tokens_delta",
    ] {
        assert!(
            metrics["deltas"][key].as_f64().is_some(),
            "missing compare delta {key}"
        );
    }
}

struct Gate2Baseline {
    stale_recall_wlb: f64,
    valid_retention_wlb: f64,
    latency_p50_ms: u64,
}

/// Load v1.1 pilot no-regression floors from the committed metrics
/// artifact. Using the JSON artifact as the source of truth avoids
/// f64 literal roundoff mismatches when a v1.2a metric equals v1.1.
fn load_v1_1_gate2_baseline(metrics_path: &Path) -> std::io::Result<Gate2Baseline> {
    let metrics: serde_json::Value = serde_json::from_slice(&std::fs::read(metrics_path)?)
        .expect("v1.1 metrics.json must be JSON");
    let col = metrics
        .get("phase1_rules_v11")
        .or_else(|| metrics.get("phase1_rules"))
        .expect("v1.1 phase1 rules column");

    Ok(Gate2Baseline {
        stale_recall_wlb: col["section_7_1"]["stale_detection"]["wilson_lower_95"]
            .as_f64()
            .expect("v1.1 stale recall WLB"),
        valid_retention_wlb: col["section_7_1"]["valid_retention_accuracy"]["wilson_lower_95"]
            .as_f64()
            .expect("v1.1 valid retention WLB"),
        latency_p50_ms: col["section_7_2_applicable"]["latency_p50_ms"]
            .as_u64()
            .expect("v1.1 latency p50"),
    })
}

/// Count `stalesourcechanged__valid` rows whose corresponding fact has
/// `kind = "Field"`. Joins a phase1 predictions.jsonl artifact with the
/// facts file used for the run.
fn count_stalesourcechanged_valid_field(
    predictions_path: &Path,
    facts_path: &Path,
) -> std::io::Result<usize> {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader};

    let mut kind_by_fact: HashMap<String, String> = HashMap::new();
    let facts_f = std::fs::File::open(facts_path)?;
    for line in BufReader::new(facts_f).lines() {
        let line = line?;
        let v: serde_json::Value =
            serde_json::from_str(&line).expect("facts.jsonl row must be JSON");
        let fid = v["fact_id"].as_str().expect("fact_id");
        let kind = v["kind"].as_str().expect("kind");
        kind_by_fact.insert(fid.to_string(), kind.to_string());
    }

    let mut count = 0usize;
    let preds_f = std::fs::File::open(predictions_path)?;
    for line in BufReader::new(preds_f).lines() {
        let line = line?;
        let v: serde_json::Value =
            serde_json::from_str(&line).expect("predictions.jsonl row must be JSON");
        let gt = v["ground_truth"].as_str().unwrap_or("");
        let pred = v["prediction"].as_str().unwrap_or("");
        if gt == "StaleSourceChanged" && pred == "valid" {
            let fid = v["fact_id"].as_str().unwrap_or("");
            if kind_by_fact.get(fid).map(|s| s.as_str()) == Some("Field") {
                count += 1;
            }
        }
    }
    Ok(count)
}

fn ensure_scoring_binary_built() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scoring_manifest = manifest_dir.join("../scoring/Cargo.toml");
    let bin_path = manifest_dir.join("../scoring/target/release/provbench-score");

    // Always rebuild: a stale release binary can make this test exercise an
    // older compare schema after `provbench-scoring` changes.
    let status = std::process::Command::new("cargo")
        .args([
            "build",
            "--release",
            "--manifest-path",
            scoring_manifest.to_str().unwrap(),
            "--bin",
            "provbench-score",
        ])
        .status()
        .expect("failed to spawn cargo build for provbench-score");
    assert!(
        status.success(),
        "cargo build --release --manifest-path benchmarks/provbench/scoring/Cargo.toml failed; cannot run §8 gate"
    );
    assert!(
        bin_path.exists(),
        "provbench-score binary not found at {} even after cargo build",
        bin_path.display()
    );
    bin_path
}
