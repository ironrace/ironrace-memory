use provbench_phase1::diffs::CommitDiff;
use provbench_phase1::facts::FactBody;
use provbench_phase1::rules::r7_rename_candidate::R7RenameCandidate;
use provbench_phase1::rules::{Decision, RowCtx, Rule, RuleChain};

fn ctx<'a>(
    fact: &'a FactBody,
    post_blob: Option<&'a [u8]>,
    t0_blob: Option<&'a [u8]>,
) -> RowCtx<'a> {
    RowCtx {
        fact,
        commit_sha: "0000",
        diff: None,
        post_blob,
        t0_blob,
        post_tree: None,
        commit_files: &[],
    }
}

fn fact(kind: &str, content_hash: &str) -> FactBody {
    FactBody {
        fact_id: "f".into(),
        kind: kind.into(),
        body: "b".into(),
        source_path: "src/lib.rs".into(),
        line_span: [10, 12],
        symbol_path: "foo".into(),
        content_hash_at_observation: content_hash.into(),
        labeler_git_sha: "deadbeef".into(),
    }
}

#[test]
fn r1_file_missing_fires_before_r2() {
    // file missing -> Stale (stale_source_deleted)
    let chain = RuleChain::default();
    let f = fact("FunctionSignature", "x");
    let (d, rid, _spec, _ev) = chain.classify_first_match(&ctx(&f, None, Some(b"original")));
    assert_eq!(d, Decision::Stale);
    assert_eq!(rid, "R1");
}

#[test]
fn r2_blob_identical_fires_before_r4() {
    // file present, blob hash identical to T0 -> Valid
    let chain = RuleChain::default();
    let f = fact("FunctionSignature", "x");
    let blob = b"hello\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(blob), Some(blob)));
    assert_eq!(d, Decision::Valid);
    assert_eq!(rid, "R2");
}

#[test]
fn r9_fallback_fires_last() {
    // No specialist rule fires (non-symbol kind, non-DocClaim, span hash matches
    // observation, blobs differ so R2 misses, no whitespace-equivalence). R9
    // should catch the fallthrough.
    let chain = RuleChain::default();
    // sha256("") for an out-of-range line_span span extraction.
    let empty_span_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let f = fact("Other", empty_span_hash);
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(b"changed"), Some(b"original")));
    assert_eq!(d, Decision::NeedsRevalidation);
    assert_eq!(rid, "R9");
}

#[test]
fn r5_whitespace_or_comment_only_fires_before_r4_for_rust() {
    let chain = RuleChain::default();
    let f = fact("FunctionSignature", "x");
    let t0 = b"fn foo() -> u32 { 42 }\n";
    let mod_ = b"fn foo() -> u32 {\n    // re-formatted, comment added\n    42\n}\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(mod_), Some(t0)));
    assert_eq!(d, Decision::Valid);
    assert_eq!(rid, "R5");
}

#[test]
fn r3_symbol_missing_when_file_present_but_symbol_gone() {
    let chain = RuleChain::default();
    let mut f = fact("FunctionSignature", "x");
    f.symbol_path = "foo".into();
    let post = b"fn bar() {}\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(b"fn foo() {}\n")));
    assert_eq!(d, Decision::Stale);
    assert_eq!(rid, "R3");
}

/// SPEC §10 pilot tuning: R3's substring search uses the **leaf**
/// symbol name (last `::`-separated component) — not the fully qualified
/// path. Rust source never literally contains `Type::field` because the
/// field is declared inside its parent struct on a separate line.
/// Falling through here lets R4 (line-blob compare) make the call.
#[test]
fn r3_uses_leaf_symbol_name_for_substring_search() {
    let chain = RuleChain::default();
    let mut f = fact("Field", "x");
    // Qualified path: leaf is `dir`.
    f.symbol_path = "IgnoreInner::dir".into();
    f.line_span = [1, 1];
    // post_blob contains the leaf `dir` but not the qualified form.
    let post = b"    dir: PathBuf,\n";
    let t0 = b"    dir: PathBuf,\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(t0)));
    // R2 fires because blobs are byte-identical here. The contract under
    // test is that R3 does NOT fire (would be `Stale` with rid="R3").
    assert_ne!(
        rid, "R3",
        "R3 should fall through when leaf symbol is present"
    );
    assert_eq!(d, Decision::Valid);
}

/// R3 still fires when even the leaf is gone from the post blob.
#[test]
fn r3_fires_when_leaf_symbol_is_absent() {
    let chain = RuleChain::default();
    let mut f = fact("FunctionSignature", "x");
    f.symbol_path = "Type::locations_mut".into();
    // post_blob has the type but not the leaf.
    let post = b"struct Type;\nimpl Type { fn other(&self) {} }\n";
    let t0 = b"struct Type;\nimpl Type { fn locations_mut(&self) {} }\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(t0)));
    assert_eq!(d, Decision::Stale);
    assert_eq!(rid, "R3");
}

#[test]
fn r4_fires_when_span_hash_changes_no_whitespace_only_escape() {
    let chain = RuleChain::default();
    let mut f = fact(
        "FunctionSignature",
        "ee26b0dd4af7e749aa1a8ee3c10ae9923f618980772e473f8819a5d4940e0db27ac185f8a0e1d5f84f88bc887fd67b143732c304cc5fa9ad8e6f57f50028a8ff",
    );
    f.symbol_path = "foo".into();
    f.line_span = [1, 1];
    let post = b"fn foo() -> u64 { 1 }\n";
    let (d, rid, _, _) =
        chain.classify_first_match(&ctx(&f, Some(post), Some(b"fn foo() -> u32 { 1 }\n")));
    assert_eq!(d, Decision::Stale);
    assert!(rid == "R4" || rid == "R3" || rid == "R7");
}

/// SPEC §10 pilot tuning: R4 cannot reproduce the labeler's
/// `content_hash_at_observation` (the labeler hashes a sub-line
/// byte_range; phase1 only has line_span). New R4 contract:
/// extract T0 lines[start..=end] and search for that byte sequence in
/// post. The probe must (a) contain the symbol's leaf identifier and
/// (b) have ≥ MIN_PROBE_NONWS_LEN non-whitespace bytes — otherwise
/// trivial lines like `}` or `#[test]` collapse every fact onto the
/// same probe.
///   - probe present in post → Valid (lines may have shifted but the
///     code is byte-stable).
///   - probe absent (or too noisy) → Stale.
///
/// R5/R6 still get first crack at whitespace-only and doc cases.
#[test]
fn r4_valid_when_t0_span_appears_unchanged_in_post() {
    let chain = RuleChain::default();
    let mut f = fact("Field", "irrelevant_hash");
    f.symbol_path = "dir_field".into();
    f.line_span = [4, 4];
    // The fact's line at T0 is "    dir_field: PathBuf," — that line
    // contains the leaf "dir_field" and is ≥ 8 non-ws bytes. In post,
    // a new field has been inserted above so the fact's line now
    // lives at line 5; the byte sequence is still present.
    let t0 =
        b"struct S {\n    a: u8,\n    b: u8,\n    dir_field: PathBuf,\n    c: u8,\n    d: u8,\n}\n";
    let post = b"struct S {\n    new_field: bool,\n    a: u8,\n    b: u8,\n    dir_field: PathBuf,\n    c: u8,\n    d: u8,\n}\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(t0)));
    assert_eq!(d, Decision::Valid);
    assert_eq!(rid, "R4");
}

#[test]
fn r4_stale_when_post_span_lines_differ_from_t0() {
    let chain = RuleChain::default();
    let mut f = fact("Field", "irrelevant_hash");
    f.symbol_path = "dir_field".into();
    f.line_span = [4, 4];
    // Leaf "dir_field" present in T0 line and probe is long enough; in
    // post the dir_field line was modified so the byte sequence is
    // not present.
    let t0 =
        b"struct S {\n    a: u8,\n    b: u8,\n    dir_field: PathBuf,\n    c: u8,\n    d: u8,\n}\n";
    let post =
        b"struct S {\n    a: u8,\n    b: u8,\n    dir_field: String,\n    c: u8,\n    d: u8,\n}\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(t0)));
    assert_eq!(d, Decision::Stale);
    assert_eq!(rid, "R4");
}

/// v1.2 Field-guard fix: a single-character struct field at T0 like
/// `    c: C,\n` has nonws_len = 4, which the v1.1 R4 length floor
/// (MIN_PROBE_NONWS_LEN = 8) rejects — routing the row to Stale even
/// though the byte sequence is still present in post. v1.2 drops the
/// length floor for kind = "Field" (keeping `probe_has_leaf` as a
/// sanity floor); R4 must now classify this as Valid.
#[test]
fn r4_valid_when_short_field_probe_appears_unchanged_in_post() {
    let chain = RuleChain::default();
    let mut f = fact("Field", "irrelevant_hash");
    f.symbol_path = "S::c".into();
    f.line_span = [3, 3];
    let t0 = b"struct S {\n    a: A,\n    c: C,\n    d: D,\n}\n";
    // post inserts a new field above `c` so the line shifts but the
    // byte sequence `    c: C,\n` is still present verbatim.
    let post = b"struct S {\n    a: A,\n    b: B,\n    c: C,\n    d: D,\n}\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(t0)));
    assert_eq!(d, Decision::Valid);
    assert_eq!(rid, "R4");
}

/// v1.2 safety check for the dropped Field length floor: a short Field
/// probe whose byte sequence is NOT present in post must still route
/// to Stale, not silently fall through. Without this guarantee, the
/// `Field` arm would null-match degenerate post blobs.
#[test]
fn r4_stale_when_short_field_probe_absent_from_post() {
    let chain = RuleChain::default();
    let mut f = fact("Field", "irrelevant_hash");
    f.symbol_path = "S::c".into();
    f.line_span = [3, 3];
    let t0 = b"struct S {\n    a: A,\n    c: C,\n    d: D,\n}\n";
    // post replaces field `c: C,` with `c: NewType,` — the original
    // byte sequence `    c: C,\n` is gone.
    let post = b"struct S {\n    a: A,\n    c: NewType,\n    d: D,\n}\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(t0)));
    assert_eq!(d, Decision::Stale);
    assert_eq!(rid, "R4");
}

/// R7 fires when the source file is gone at the commit but a same-extension
/// candidate in the commit tree has a basename whose token-Jaccard similarity
/// against the qualified-symbol leaf clears the 0.6 threshold. With the
/// new leaf-vs-stem proxy (replacing the previous body-vs-path proxy), the
/// minimum-viable rename case is "stem == leaf" (similarity = 1.0).
#[test]
fn r7_fires_when_leaf_matches_renamed_file_stem() {
    let mut f = fact("FunctionSignature", "x");
    f.symbol_path = "Walk::new".into();
    f.source_path = "src/walker.rs".into();
    let cf = vec![
        "src/other.rs".to_string(),
        "src/new.rs".to_string(),
        "src/util.rs".to_string(),
    ];
    let ctx = RowCtx {
        fact: &f,
        commit_sha: "0000",
        diff: None,
        post_blob: None, // R7 requires post_blob.is_none()
        t0_blob: Some(b"unused"),
        post_tree: None,
        commit_files: &cf,
    };
    let r7 = R7RenameCandidate;
    let result = r7.classify(&ctx).expect("R7 should fire on stem == leaf");
    assert_eq!(result.0, Decision::Stale);
    assert!(
        result.1.contains(r#""to":"src/new.rs""#),
        "expected R7 to pick src/new.rs (stem 'new' == leaf 'new'); got: {}",
        result.1
    );
    assert!(
        result.1.contains(r#""leaf":"new""#),
        "expected leaf evidence; got: {}",
        result.1
    );
}

/// R7 ignores same-extension paths whose stem doesn't share enough tokens
/// with the leaf. Single-token Jaccard means anything other than an exact
/// match scores 0, so this is the typical no-fire case.
#[test]
fn r7_does_not_fire_without_matching_stem() {
    let mut f = fact("FunctionSignature", "x");
    f.symbol_path = "Walk::new".into();
    f.source_path = "src/walker.rs".into();
    let cf = vec![
        "src/something_else.rs".to_string(),
        "src/util.rs".to_string(),
    ];
    let ctx = RowCtx {
        fact: &f,
        commit_sha: "0000",
        diff: None,
        post_blob: None,
        t0_blob: Some(b"unused"),
        post_tree: None,
        commit_files: &cf,
    };
    let r7 = R7RenameCandidate;
    assert!(
        r7.classify(&ctx).is_none(),
        "R7 fired on a tree with no leaf-match — heuristic should be conservative"
    );
}

/// R7 requires same extension as the original source — a Python file
/// matching the leaf does not count as a rename of a Rust symbol.
#[test]
fn r7_requires_extension_match() {
    let mut f = fact("FunctionSignature", "x");
    f.symbol_path = "Walk::new".into();
    f.source_path = "src/walker.rs".into();
    let cf = vec!["src/new.py".to_string(), "src/new".to_string()];
    let ctx = RowCtx {
        fact: &f,
        commit_sha: "0000",
        diff: None,
        post_blob: None,
        t0_blob: Some(b"unused"),
        post_tree: None,
        commit_files: &cf,
    };
    let r7 = R7RenameCandidate;
    assert!(
        r7.classify(&ctx).is_none(),
        "R7 fired on extension-mismatched candidates: cross-language rename is not a v1 case"
    );
}

/// Tie-break contract: when multiple same-extension paths have a stem
/// equal to the leaf (similarity = 1.0 each), R7 picks the
/// alphabetically smallest path. Presenting candidates in reverse
/// alphabetical order locks the ordering: a bug that just picks the
/// last-seen winner would let "src/z/new.rs" through.
#[test]
fn r7_tie_break_picks_alphabetically_smaller_path() {
    let mut f = fact("FunctionSignature", "x");
    f.symbol_path = "Walk::new".into();
    f.source_path = "src/walker.rs".into();
    // Two candidates whose stem is exactly "new" with the same extension.
    // Both score similarity = 1.0 against leaf "new"; tie-break picks the
    // alphabetically smallest path. Presented in reverse order to catch
    // a naive last-wins bug.
    let cf = vec!["src/z/new.rs".to_string(), "src/a/new.rs".to_string()];
    let ctx = RowCtx {
        fact: &f,
        commit_sha: "0000",
        diff: None,
        post_blob: None,
        t0_blob: Some(b"unused"),
        post_tree: None,
        commit_files: &cf,
    };
    let r7 = R7RenameCandidate;
    let result = r7.classify(&ctx).expect("R7 should fire");
    assert_eq!(result.0, Decision::Stale);
    assert!(
        result.1.contains(r#""to":"src/a/new.rs""#),
        "tie-break inversion: expected alphabetically smaller 'src/a/new.rs', got: {}",
        result.1
    );
}

/// R7 now sits ahead of R1 in `RuleChain::default()`. When the chain
/// runs against a deleted file and the commit tree contains a clear
/// rename candidate, R7 wins (Stale + R7); when no candidate matches,
/// R1 still fires (Stale + R1). The two paths are mutually exclusive.
#[test]
fn r7_pre_empts_r1_when_rename_candidate_present() {
    let chain = RuleChain::default();
    let mut f = fact("FunctionSignature", "x");
    f.symbol_path = "Walk::new".into();
    f.source_path = "src/walker.rs".into();
    let cf = vec!["src/new.rs".to_string()];
    let ctx = RowCtx {
        fact: &f,
        commit_sha: "0000",
        diff: None,
        post_blob: None,
        t0_blob: Some(b"unused"),
        post_tree: None,
        commit_files: &cf,
    };
    let (d, rid, _, _) = chain.classify_first_match(&ctx);
    assert_eq!(d, Decision::Stale);
    assert_eq!(
        rid, "R7",
        "R7 should pre-empt R1 when a rename candidate exists; chain order may have regressed"
    );
}

#[test]
fn r0_diff_excluded_fires_for_orphan_diff() {
    // R0 guard: excluded_reason.is_some() && post_blob.is_none() && !commit_files.is_empty()
    // Use kind "Other" so neither R3 nor R7 fires (both require FunctionSignature/Field/PublicSymbol).
    // R1 requires t0_blob.is_some() && post_blob.is_none(); to keep R0 ahead of R1, R0 is
    // already declared before R1 in the chain, so as long as the guard holds, R0 wins.
    let chain = RuleChain::default();
    let cd = CommitDiff {
        commit_sha: "abc123".into(),
        parent_sha: None,
        excluded_reason: Some("orphan".into()),
        unified_diff: None,
    };
    let f = fact("Other", "x");
    let cf = vec!["some/path.rs".to_string()];
    let ctx = RowCtx {
        fact: &f,
        commit_sha: "abc123",
        diff: Some(&cd),
        post_blob: None,
        t0_blob: Some(b"original"),
        post_tree: None,
        commit_files: &cf,
    };
    let (d, rid, _, _) = chain.classify_first_match(&ctx);
    assert_eq!(d, Decision::NeedsRevalidation);
    assert_eq!(rid, "R0");
}

#[test]
fn r6_doc_claim_symbol_still_mentioned_is_valid() {
    let chain = RuleChain::default();
    let mut f = fact("DocClaim", "x");
    f.symbol_path = "foo".into();
    let post = b"This page mentions foo at length.\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(b"older content\n")));
    assert_eq!(d, Decision::Valid);
    assert_eq!(rid, "R6");
}
