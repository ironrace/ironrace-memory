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

// ── Pass-3 hardening regressions (diff level) ────────────────────────────────
//
// HP3-3b and HP3-3 are RED against HEAD a6b7e5c (Cluster C rename false
// positives) and carry `#[ignore = "RED until Task 4 …"]` so that
// `cargo test` stays green.
// HP3-4 is GREEN on HEAD (a preservation contract for Task 4).
//
// HP3-2 (Cluster B per-commit symbol resolution) is an integration-level
// test in replay_hardening.rs.  The diff-level pin for the rename heuristic
// side of that scenario is here as HP3-3b (Cluster C, Task 4).

// ── HP3-3b: rename candidate triggers for replace_with_captures → replace_with_caps

/// `fn replace_with_captures(s: &str) -> String` vs a post-commit pool that
/// contains only `fn replace_with_caps(s: &str) -> String`.
///
/// The two spans are structurally near-identical (same signature shape, same
/// body, only the trailing `ures` characters differ in the name).  With the
/// current `TextDiff::ratio` heuristic at threshold 0.6 the candidate fires
/// and returns `Some("replace_with_caps")` — a false positive.
///
/// **Why it fails on HEAD a6b7e5c:**
/// `TextDiff::from_chars` counts character-level similarity across the full
/// span bytes.  `replace_with_captures` and `replace_with_caps` differ in
/// only four characters out of a long identical suffix, pushing the ratio
/// well above 0.6.  A correctly-hardened heuristic must reject this case
/// (the function was deleted, not renamed; the new symbol is an independent
/// addition).
#[test]
#[ignore = "RED until Task 4 (cluster C rename false positive)"]
fn hp3_3b_replacement_deletion_no_false_rename_candidate() {
    let before = b"pub fn replace_with_captures(s: &str) -> String { s.to_string() }";
    let after_candidates = vec![(
        "replace_with_caps".to_string(),
        b"pub fn replace_with_caps(s: &str) -> String { s.to_string() }".to_vec(),
    )];
    let result = rename_candidate(before, &after_candidates, 0.6);
    assert_eq!(
        result, None,
        "replace_with_captures → replace_with_caps must NOT be treated as a \
         rename (the original was deleted; the similar name is an independent \
         addition), but rename_candidate returned {:?}",
        result
    );
}

// ── HP3-3 (Cluster C): field-drop rename false positive ──────────────────────

/// T0 field span: `all_verbatim_literal: bool`.
/// Post-commit candidate pool: `any_literal: bool` (the field was dropped,
/// not renamed; `any_literal` was already present in the struct at T0).
///
/// **Why it fails on HEAD a6b7e5c:**
/// `TextDiff::from_chars` on these two short spans produces a ratio above 0.6
/// because both share the `_literal: bool` suffix.  The heuristic treats this
/// as a rename and returns `Some("AstAnalysis::any_literal")`.  The correct
/// result is `None` — the field was deleted, and `any_literal` is a distinct,
/// pre-existing field whose qualified name is unrelated.
#[test]
#[ignore = "RED until Task 4 (cluster C rename false positive)"]
fn hp3_3_field_drop_no_false_rename_candidate() {
    // Field spans as they appear in the AST byte slice used by
    // `rename_candidates_for` (just the field content, no surrounding struct).
    let before = b"all_verbatim_literal: bool";
    let after_candidates = vec![(
        "AstAnalysis::any_literal".to_string(),
        b"any_literal: bool".to_vec(),
    )];
    let result = rename_candidate(before, &after_candidates, 0.6);
    assert_eq!(
        result, None,
        "dropping `all_verbatim_literal` with `any_literal` remaining must NOT \
         be treated as a rename (they are distinct fields; the suffix `_literal: \
         bool` inflates similarity), but rename_candidate returned {:?}",
        result
    );
}

// ── HP3-4: rename true positive (preservation) ──────────────────────────────

/// `fn locations_mut(&mut self) -> …` becomes `fn captures_mut(&mut self) -> …`
/// in the same `impl AutomataCaptures` block.
///
/// The spans are near-identical (same return type, same body, only the
/// function name differs).  `TextDiff::ratio ≥ 0.6` must fire and return
/// `Some("captures_mut")`.
///
/// This test is GREEN on HEAD a6b7e5c and must remain GREEN through Task 4.
/// If Task 4's heuristic changes cause it to return `None`, that task must
/// document the regression and add a SPEC §11 entry for the rule change.
#[test]
fn hp3_4_rename_true_positive_same_impl_is_detected() {
    let before = b"fn locations_mut(&mut self) -> &mut Vec<Location> { &mut self.locations }";
    let after_candidates = vec![(
        "captures_mut".to_string(),
        b"fn captures_mut(&mut self) -> &mut Vec<Location> { &mut self.locations }".to_vec(),
    )];
    let result = rename_candidate(before, &after_candidates, 0.6);
    assert_eq!(
        result.as_deref(),
        Some("captures_mut"),
        "locations_mut → captures_mut in the same impl block must be detected \
         as a rename (same-container, near-identical span), but rename_candidate \
         returned {:?}",
        result
    );
}
