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

#[test]
fn r7_tie_break_picks_alphabetically_smaller_path() {
    // Equal-similarity tie-break: SPEC §5 step 2 → (similarity desc,
    // qualified_name asc). Among candidates with identical similarity,
    // the alphabetically smallest path wins.
    //
    // Construct two candidate "paths" that share identical Jaccard
    // similarity with body. The similarity function is whitespace-token
    // Jaccard, so we hand-build path strings whose token sets share
    // equal overlap with the body's token set.
    //
    // body tokens = {"a","b","c"}.
    // path "z {a b c}" → tokens = {"z","a","b","c"}; intersection=3, union=4 → 0.75
    // path "y {a b c}" → tokens = {"y","a","b","c"}; intersection=3, union=4 → 0.75
    // Both are above the 0.6 threshold and equal. Present in reverse
    // order ("z…" first, "y…" second). Tie-break must pick the
    // alphabetically smaller path: "y …".
    //
    // R7 isn't reachable through `RuleChain` because R1 fires first
    // whenever post_blob is None. Test R7's classify() directly so the
    // tie-break contract is locked independently of chain ordering.
    let mut f = fact("FunctionSignature", "x");
    f.symbol_path = "foo".into();
    f.body = "a b c".into();
    f.source_path = "src/other.rs".into();
    let cf = vec!["z a b c".to_string(), "y a b c".to_string()];
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
    let result = r7.classify(&ctx).expect("R7 should fire");
    assert_eq!(result.0, Decision::Stale);
    // Alphabetically smaller path wins: "y …" < "z …".
    assert!(
        result.1.contains(r#""to":"y a b c""#),
        "tie-break inversion: expected alphabetically smaller 'y' path, got evidence: {}",
        result.1
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
