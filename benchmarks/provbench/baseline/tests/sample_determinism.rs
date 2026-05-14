//! Same seed + same inputs → byte-identical manifest (acceptance #1, #2, #5).
//!
//! NOTE: both `m1` and `m2` are constructed in the same test invocation
//! against the same git HEAD, so `baseline_crate_head_sha` agrees. If
//! this test ever runs across two different commits it would fail —
//! that's intentional provenance, not flakiness.

use provbench_baseline::manifest::SampleManifest;
use provbench_baseline::sample::PerStratumTargets;
use std::path::Path;

#[test]
fn same_seed_yields_byte_identical_manifest() {
    let targets = PerStratumTargets::default();
    let m1 = SampleManifest::from_corpus(
        Path::new("fixtures/sample_corpus.jsonl"),
        Path::new("fixtures/sample_facts.jsonl"),
        Path::new("fixtures/sample_diffs"),
        0xC0DE_BABE_DEAD_BEEF,
        targets.clone(),
        25.0,
    )
    .expect("manifest 1 builds");
    let m2 = SampleManifest::from_corpus(
        Path::new("fixtures/sample_corpus.jsonl"),
        Path::new("fixtures/sample_facts.jsonl"),
        Path::new("fixtures/sample_diffs"),
        0xC0DE_BABE_DEAD_BEEF,
        targets,
        25.0,
    )
    .expect("manifest 2 builds");

    assert_eq!(m1.canonical_json(), m2.canonical_json());
    assert_eq!(m1.content_hash, m2.content_hash);
    assert_eq!(m1.selected_count, m2.selected_count);
    assert!(
        m1.selected_count > 0,
        "fixture must produce at least one selected row"
    );
}
