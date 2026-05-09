use provbench_labeler::label::Label;
use provbench_labeler::output::OutputRow;
use provbench_labeler::spotcheck::sample;

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
