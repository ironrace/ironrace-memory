use provbench_labeler::diff::is_whitespace_or_comment_only;

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
