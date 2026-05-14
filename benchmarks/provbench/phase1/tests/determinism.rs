use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// Two runs over the same --baseline-run subset must produce byte-identical
/// predictions.jsonl. (wall_ms is non-deterministic; this test runs the
/// score CLI twice and asserts the non-wall_ms fields match for every row.)
#[test]
fn predictions_jsonl_is_byte_stable_modulo_wall_ms() {
    let provbench = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let workrepo = provbench.join("work/ripgrep");
    if !workrepo.exists() {
        eprintln!("skipping determinism test: work/ripgrep not present");
        return;
    }

    let bin = env!("CARGO_BIN_EXE_provbench-phase1");
    let run = |out: &PathBuf| {
        let status = Command::new(bin)
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
                out.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success(), "phase1 score failed");
    };

    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let pa = a.path().to_path_buf();
    let pb = b.path().to_path_buf();
    run(&pa);
    run(&pb);

    let read_rows = |p: &PathBuf| -> Vec<serde_json::Value> {
        let s = std::fs::read_to_string(p.join("predictions.jsonl")).unwrap();
        s.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    };
    let ra = read_rows(&pa);
    let rb = read_rows(&pb);
    assert_eq!(ra.len(), rb.len());
    for (x, y) in ra.iter().zip(rb.iter()) {
        for f in [
            "fact_id",
            "commit_sha",
            "batch_id",
            "ground_truth",
            "prediction",
            "request_id",
        ] {
            assert_eq!(x[f], y[f], "field {} differs across runs", f);
        }
    }
}
