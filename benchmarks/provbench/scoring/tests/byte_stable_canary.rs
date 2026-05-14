use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Reproduces benchmarks/provbench/results/phase0c/2026-05-13-canary/metrics.json
/// byte-for-byte by re-running the shared scorer over its own predictions.jsonl.
/// Locks the SPEC §6.2/§7 math against accidental drift during the
/// baseline -> scoring extraction.
#[test]
fn phase0c_canary_metrics_byte_stable() {
    let canary =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../results/phase0c/2026-05-13-canary");

    let tmp = TempDir::new().unwrap();
    for name in ["manifest.json", "predictions.jsonl", "run_meta.json"] {
        fs::copy(canary.join(name), tmp.path().join(name)).unwrap();
    }

    provbench_scoring::report::score_llm_baseline_run(tmp.path()).unwrap();

    let got = fs::read(tmp.path().join("metrics.json")).unwrap();
    let want = fs::read(canary.join("metrics.json")).unwrap();
    assert_eq!(got, want, "metrics.json byte-stable canary regressed");
}
