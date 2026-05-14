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
}

fn ensure_scoring_binary_built() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scoring_manifest = manifest_dir.join("../scoring/Cargo.toml");
    let bin_path = manifest_dir.join("../scoring/target/release/provbench-score");

    // Build the scoring crate's provbench-score binary if not already present.
    // This makes the §8 gate test self-contained: `cargo test --ignored` just works.
    if !bin_path.exists() {
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
            "cargo build --release -p provbench-scoring failed; cannot run §8 gate"
        );
    }
    assert!(
        bin_path.exists(),
        "provbench-score binary not found at {} even after cargo build",
        bin_path.display()
    );
    bin_path
}
