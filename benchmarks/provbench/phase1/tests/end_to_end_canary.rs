use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

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
