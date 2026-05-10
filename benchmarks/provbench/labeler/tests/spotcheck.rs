use provbench_labeler::label::Label;
use provbench_labeler::output::OutputRow;
use provbench_labeler::spotcheck::{sample, wilson_lower_bound_95};

#[test]
fn deterministic_sampler_returns_same_indices_across_runs() {
    let rows: Vec<OutputRow> = (0..1000)
        .map(|i| OutputRow {
            fact_id: format!("f{i}"),
            commit_sha: format!("c{}", i % 10),
            label: if i % 5 == 0 {
                Label::StaleSourceChanged
            } else {
                Label::Valid
            },
        })
        .collect();
    let s1 = sample(&rows, 200);
    let s2 = sample(&rows, 200);
    assert_eq!(s1.len(), 200);
    assert_eq!(s1, s2);
}

#[test]
fn rare_classes_meet_min_floor() {
    let rows: Vec<OutputRow> = (0..1000)
        .map(|i| OutputRow {
            fact_id: format!("f{i}"),
            commit_sha: format!("c{i}"),
            label: match i % 100 {
                0..=1 => Label::StaleSymbolRenamed {
                    new_name: "x".into(),
                },
                2..=3 => Label::StaleSourceDeleted,
                _ => Label::Valid,
            },
        })
        .collect();
    let s = sample(&rows, 200);
    let renamed = s
        .iter()
        .filter(|r| matches!(r.row.label, Label::StaleSymbolRenamed { .. }))
        .count();
    assert!(renamed >= 10, "rare class under-sampled: got {renamed}");
}

#[test]
fn wilson_lower_bound_at_perfect_score() {
    let lb = wilson_lower_bound_95(200, 200);
    assert!(lb > 0.98, "got {lb}");
}

#[test]
fn wilson_lower_bound_at_95_point_estimate() {
    let lb = wilson_lower_bound_95(190, 200);
    // analytic: ~0.910
    assert!(lb > 0.90 && lb < 0.93, "got {lb}");
}

#[test]
fn wilson_lower_bound_at_perfect_score_is_above_correct_threshold() {
    let lb = wilson_lower_bound_95(199, 200);
    assert!((lb - 0.972_226_295_6).abs() < 0.000_05, "got {lb}");
}

#[test]
fn wilson_lower_bound_zero_total_returns_zero() {
    assert_eq!(wilson_lower_bound_95(0, 0), 0.0);
}
