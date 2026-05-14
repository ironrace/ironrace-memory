//! End-to-end fixture smoke test for the `provbench-baseline` CLI.
//!
//! Runs the full `sample` → `run` → `score` pipeline against the canned
//! Task 5 fixtures in `fixtures/` using `--dry-run` so no network access
//! or canned API responses are required. Verifies that each subcommand
//! produces its on-disk artifact with the expected top-level schema.
//!
//! Working directory at `cargo test` time is the crate root
//! (`benchmarks/provbench/baseline/`), so the `fixtures/...` paths
//! resolve relative to that.

use std::process::Command;
use tempfile::TempDir;

#[test]
fn full_pipeline_against_fixtures_produces_all_artifacts() {
    let bin = env!("CARGO_BIN_EXE_provbench-baseline");
    let tmp = TempDir::new().unwrap();
    let run_dir = tmp.path().join("run");
    std::fs::create_dir_all(&run_dir).unwrap();

    // 1. sample
    let m_status = Command::new(bin)
        .args([
            "sample",
            "--corpus",
            "fixtures/sample_corpus.jsonl",
            "--facts",
            "fixtures/sample_facts.jsonl",
            "--diffs-dir",
            "fixtures/sample_diffs",
            "--out",
            run_dir.join("manifest.json").to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(m_status.success(), "sample subcommand failed");
    assert!(run_dir.join("manifest.json").exists());

    // Read manifest to confirm selected_count > 0.
    let manifest_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run_dir.join("manifest.json")).unwrap()).unwrap();
    let selected_count = manifest_json["selected_count"].as_u64().unwrap();
    assert!(selected_count > 0, "manifest must select at least one row");

    // 2. run (dry-run → no network, no fixtures needed)
    let r_status = Command::new(bin)
        .args([
            "run",
            "--manifest",
            run_dir.join("manifest.json").to_str().unwrap(),
            "--dry-run",
        ])
        .status()
        .unwrap();
    assert!(r_status.success(), "run subcommand failed");
    assert!(run_dir.join("predictions.jsonl").exists());
    assert!(run_dir.join("run_meta.json").exists());

    // Count prediction rows: must match the manifest's selected_count.
    let predictions = std::fs::read_to_string(run_dir.join("predictions.jsonl")).unwrap();
    let prediction_lines = predictions.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(
        prediction_lines as u64, selected_count,
        "predictions.jsonl row count must match manifest.selected_count"
    );

    // 3. score
    let s_status = Command::new(bin)
        .args(["score", "--run", run_dir.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(s_status.success(), "score subcommand failed");

    // Validate metrics.json schema.
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run_dir.join("metrics.json")).unwrap()).unwrap();
    assert_eq!(m["coverage"], "subset");
    assert!(
        m["section_7_1"]["stale_detection"]["precision"].is_number(),
        "section_7_1.stale_detection.precision must be a number"
    );
    assert!(
        m["section_7_2_applicable"]["latency_p50_ms"].is_number(),
        "latency_p50_ms must be a number"
    );
    assert!(
        m["llm_validator_agreement"]["cohen_kappa"]["point_estimate"].is_number(),
        "cohen_kappa.point_estimate must be a number"
    );
    assert!(m["llm_validator_agreement"]["confusion_matrix_3x3"].is_array());
}
