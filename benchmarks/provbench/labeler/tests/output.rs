use provbench_labeler::label::Label;
use provbench_labeler::output::{write_jsonl, OutputRow};

#[test]
fn rows_serialize_sorted_with_labeler_stamp() {
    let rows = vec![
        OutputRow {
            fact_id: "B".into(),
            commit_sha: "c1".into(),
            label: Label::Valid,
        },
        OutputRow {
            fact_id: "A".into(),
            commit_sha: "c2".into(),
            label: Label::Valid,
        },
        OutputRow {
            fact_id: "A".into(),
            commit_sha: "c1".into(),
            label: Label::StaleSourceChanged,
        },
    ];
    let tmp = tempfile::NamedTempFile::new().unwrap();
    write_jsonl(tmp.path(), &rows, "labelersha123").unwrap();
    let body = std::fs::read_to_string(tmp.path()).unwrap();
    let lines: Vec<_> = body.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(
        lines[0].contains(r#""fact_id":"A""#) && lines[0].contains(r#""commit_sha":"c1""#),
        "line[0] should be A/c1, got: {}",
        lines[0]
    );
    assert!(
        lines[1].contains(r#""fact_id":"A""#) && lines[1].contains(r#""commit_sha":"c2""#),
        "line[1] should be A/c2, got: {}",
        lines[1]
    );
    assert!(
        lines[2].contains(r#""fact_id":"B""#),
        "line[2] should be B, got: {}",
        lines[2]
    );
    for line in &lines {
        assert!(
            line.contains(r#""labeler_git_sha":"labelersha123""#),
            "missing labeler_git_sha stamp in: {}",
            line
        );
    }
}
