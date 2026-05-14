use std::path::PathBuf;
use tempfile::TempDir;

/// Loads the committed canary's facts + diffs + baseline-run predictions
/// into a fresh SQLite DB. Asserts the loaded counts match the artifact
/// counts (read from disk, never hard-coded) and that raw_json_sha256 is
/// deterministic across a re-ingest.
#[test]
fn load_canary_facts_diffs_evalrows() {
    let provbench = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let facts = provbench.join("facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl");
    let diffs_dir = provbench.join("facts/ripgrep-af6b6c54-c2d3b7b.diffs");
    let baseline_run = provbench.join("results/phase0c/2026-05-13-canary");

    // Unique fact_id count, computed from disk (semantic duplicates with
    // matching fields are ingested once; non-matching duplicates fail closed).
    let expected_facts = {
        let text = std::fs::read_to_string(&facts).unwrap();
        let mut ids = std::collections::HashSet::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            ids.insert(v["fact_id"].as_str().unwrap().to_string());
        }
        ids.len()
    };
    let expected_diffs = std::fs::read_dir(&diffs_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "json")
        })
        .count();
    let expected_rows = std::fs::read_to_string(baseline_run.join("predictions.jsonl"))
        .unwrap()
        .lines()
        .count();

    let tmp = TempDir::new().unwrap();
    let db = provbench_phase1::storage::open(&tmp.path().join("phase1.sqlite")).unwrap();
    provbench_phase1::facts::ingest(&db, &facts).unwrap();
    provbench_phase1::diffs::ingest(&db, &diffs_dir).unwrap();
    provbench_phase1::baseline_run::ingest(&db, &baseline_run.join("predictions.jsonl")).unwrap();

    let got_facts: i64 = db
        .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    let got_diffs: i64 = db
        .query_row("SELECT COUNT(*) FROM diff_artifacts", [], |r| r.get(0))
        .unwrap();
    let got_rows: i64 = db
        .query_row("SELECT COUNT(*) FROM eval_rows", [], |r| r.get(0))
        .unwrap();

    assert_eq!(got_facts as usize, expected_facts, "facts count mismatch");
    assert_eq!(
        got_diffs as usize, expected_diffs,
        "diff_artifacts count mismatch"
    );
    assert_eq!(got_rows as usize, expected_rows, "eval_rows count mismatch");

    // Re-ingest into a second DB; raw_json_sha256 must be identical.
    let tmp2 = TempDir::new().unwrap();
    let db2 = provbench_phase1::storage::open(&tmp2.path().join("phase1.sqlite")).unwrap();
    provbench_phase1::facts::ingest(&db2, &facts).unwrap();
    let h1: String = db
        .query_row(
            "SELECT raw_json_sha256 FROM facts WHERE fact_id = (SELECT MIN(fact_id) FROM facts)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let h2: String = db2
        .query_row(
            "SELECT raw_json_sha256 FROM facts WHERE fact_id = (SELECT MIN(fact_id) FROM facts)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        h1, h2,
        "raw_json_sha256 must be deterministic across re-ingest"
    );
}
