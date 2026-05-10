use provbench_labeler::diff::{is_whitespace_or_comment_only, rename_candidate};

#[test]
fn pure_whitespace_diff_is_ignored() {
    assert!(is_whitespace_or_comment_only(b"fn x()  {}", b"fn x() {}"));
}

#[test]
fn comment_only_diff_is_ignored() {
    assert!(is_whitespace_or_comment_only(
        b"fn x() {} // a",
        b"fn x() {} // b"
    ));
}

#[test]
fn rename_is_not_whitespace_only() {
    assert!(!is_whitespace_or_comment_only(b"fn x() {}", b"fn y() {}"));
}

#[test]
fn high_similarity_rename_is_detected() {
    let before = b"fn search_pattern(pat: &str) -> Vec<usize> { Vec::new() }";
    let after_candidates = vec![
        (
            "search_input".to_string(),
            b"fn search_input(pat: &str) -> Vec<usize> { Vec::new() }".to_vec(),
        ),
        (
            "totally_different".to_string(),
            b"fn totally_different() {}".to_vec(),
        ),
    ];
    let m = rename_candidate(before, &after_candidates, 0.6);
    assert_eq!(m.as_deref(), Some("search_input"));
}

#[test]
fn no_candidate_above_threshold_returns_none() {
    let before = b"fn alpha() {}";
    let after_candidates = vec![(
        "beta".to_string(),
        b"fn beta(x: u32, y: u32, z: u32) -> Vec<u32> { Vec::new() }".to_vec(),
    )];
    let m = rename_candidate(before, &after_candidates, 0.6);
    assert_eq!(m, None);
}
