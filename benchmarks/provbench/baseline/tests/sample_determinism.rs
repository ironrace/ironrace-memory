//! Same seed + same inputs → byte-identical `content_hash` (acceptance #1, #2, #5).
//!
//! NOTE: both `m1` and `m2` are constructed in the same test invocation
//! against the same git HEAD, so `baseline_crate_head_sha` agrees. If
//! this test ever runs across two different commits it would fail —
//! that's intentional provenance, not flakiness.
//!
//! `created_at` is wall-clock provenance and is intentionally excluded
//! from the determinism contract (see [`SampleManifest::compute_content_hash`]),
//! so `canonical_json()` may legitimately differ across runs by a
//! second; only `content_hash` is required to match.

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

    // `created_at` is provenance, not part of the determinism contract,
    // so compare every other field explicitly rather than canonical_json.
    assert_eq!(m1.content_hash, m2.content_hash);
    assert_eq!(m1.seed, m2.seed);
    assert_eq!(m1.selected_count, m2.selected_count);
    assert_eq!(
        serde_json::to_string(&m1.rows).unwrap(),
        serde_json::to_string(&m2.rows).unwrap()
    );
    assert_eq!(m1.excluded_count_by_reason, m2.excluded_count_by_reason);
    assert_eq!(m1.spec_freeze_hash, m2.spec_freeze_hash);
    assert_eq!(m1.baseline_crate_head_sha, m2.baseline_crate_head_sha);
    assert!(
        m1.selected_count > 0,
        "fixture must produce at least one selected row"
    );
}
