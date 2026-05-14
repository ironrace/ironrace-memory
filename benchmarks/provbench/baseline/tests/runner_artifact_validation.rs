use provbench_baseline::diffs::DiffArtifact;
use provbench_baseline::facts::FactBody;
use provbench_baseline::runner::build_batches;
use provbench_baseline::sample::{SampledRow, StratumKey};
use std::collections::HashMap;

fn row(fact_id: &str, commit_sha: &str) -> SampledRow {
    SampledRow {
        fact_id: fact_id.to_string(),
        commit_sha: commit_sha.to_string(),
        ground_truth: "Valid".to_string(),
        stratum: StratumKey::Valid,
    }
}

fn fact(fact_id: &str) -> FactBody {
    FactBody {
        fact_id: fact_id.to_string(),
        kind: "FunctionSignature".to_string(),
        body: "function f has parameters () with return type ()".to_string(),
        source_path: "src/lib.rs".to_string(),
        line_span: [1, 1],
        symbol_path: "f".to_string(),
        content_hash_at_observation:
            "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    }
}

#[test]
fn build_batches_fails_closed_when_selected_rows_lack_artifacts() {
    let good_commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let missing_commit = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let rows = vec![
        row("missing-fact", good_commit),
        row("present-fact", missing_commit),
    ];

    let mut facts = HashMap::new();
    facts.insert("present-fact".to_string(), fact("present-fact"));

    let mut diffs = HashMap::new();
    diffs.insert(
        good_commit.to_string(),
        DiffArtifact::Included {
            commit_sha: good_commit.to_string(),
            parent_sha: "cccccccccccccccccccccccccccccccccccccccc".to_string(),
            unified_diff: "diff --git a/src/lib.rs b/src/lib.rs\n".to_string(),
        },
    );

    let err = build_batches(&rows, &facts, &diffs).expect_err("artifact drift must fail");
    let msg = err.to_string();
    assert!(msg.contains("missing_diffs=1"), "{msg}");
    assert!(msg.contains("missing_facts=1"), "{msg}");
}
