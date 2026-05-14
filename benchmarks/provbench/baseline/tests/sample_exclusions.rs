//! Excluded rows must be counted with reason codes — never silently
//! dropped (acceptance #3).

use provbench_baseline::manifest::SampleManifest;
use provbench_baseline::sample::PerStratumTargets;
use std::path::Path;

#[test]
fn excluded_rows_are_recorded_not_silently_dropped() {
    let targets = PerStratumTargets::default();
    let m = SampleManifest::from_corpus(
        Path::new("fixtures/sample_corpus_with_exclusions.jsonl"),
        Path::new("fixtures/sample_facts.jsonl"),
        Path::new("fixtures/sample_diffs_with_t0_excluded"),
        0xC0DE_BABE_DEAD_BEEF,
        targets,
        25.0,
    )
    .expect("manifest builds");

    assert!(
        m.excluded_count_by_reason.contains_key("commit_t0"),
        "t0-excluded commit should land in excluded_count_by_reason: got {:?}",
        m.excluded_count_by_reason
    );
    assert!(
        m.excluded_count_by_reason.contains_key("missing_fact_body"),
        "fact id without a matching body row should land in excluded_count_by_reason: got {:?}",
        m.excluded_count_by_reason
    );
    let total_excluded: usize = m.excluded_count_by_reason.values().sum();
    assert!(
        total_excluded >= 2,
        "expected at least the two exclusion cases above"
    );
}
