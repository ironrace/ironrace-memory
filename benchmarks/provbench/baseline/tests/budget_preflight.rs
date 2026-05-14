use provbench_baseline::budget::preflight_worst_case_cost;
use provbench_baseline::sample::{SampledRow, StratumKey};
use std::collections::HashMap;

fn synthetic_rows(n: usize) -> Vec<SampledRow> {
    (0..n)
        .map(|i| SampledRow {
            fact_id: format!("F::{i}"),
            commit_sha: format!("{:040x}", i % 100),
            ground_truth: "Valid".into(),
            stratum: StratumKey::Valid,
        })
        .collect()
}

#[test]
fn default_n_passes_preflight_with_headroom() {
    let rows = synthetic_rows(9232);
    let diffs = HashMap::new();
    let facts = HashMap::new();
    let cost = preflight_worst_case_cost(&rows, &diffs, &facts);
    assert!(
        cost <= 17.50,
        "n=9232 must cost <= $17.50 (got ${:.2})",
        cost
    );
}

#[test]
fn oversized_manifest_exceeds_cap() {
    let rows = synthetic_rows(46_000);
    let diffs = HashMap::new();
    let facts = HashMap::new();
    let cost = preflight_worst_case_cost(&rows, &diffs, &facts);
    assert!(cost > 25.0, "5x n must exceed $25 cap (got ${:.2})", cost);
}
