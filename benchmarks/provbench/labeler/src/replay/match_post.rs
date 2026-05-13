//! Post-commit fact matching helpers.
//!
//! Given a T₀ fact and the post-commit blob/AST for its source file, these
//! functions locate the corresponding post-commit entry (if any) or enumerate
//! same-kind candidates for rename detection.  They have no dependency on
//! [`super::Replay`], [`super::CommitState`], or `run_inner` — everything
//! they need comes in through their parameters.

use crate::ast::{spans::Span, RustAst};
use crate::diff::RenameCandidate;
use crate::facts::{doc_claim, field, function_signature, symbol_existence, test_assertion, Fact};
use anyhow::Result;
use std::path::Path;

/// Search the post-commit blob/AST for the same fact (by kind + qualified
/// key).  Returns `Ok(Some((post_span, post_content_hash)))` when found,
/// `Ok(None)` when the symbol is absent, or `Err` when the post-commit
/// blob cannot be parsed (e.g. invalid UTF-8 in a markdown file).
///
/// `test_assertion_ordinal` carries the zero-based position of a
/// `Fact::TestAssertion` among same-`test_fn` siblings at T₀, computed
/// in `replay::run_inner`. It is consumed only by the `Fact::TestAssertion`
/// arm to disambiguate which post-commit assertion to compare against —
/// without it, a test fn with N assertions silently collapses to
/// assertion #1 for every match (pre-pass-4 bug). For non-TestAssertion
/// facts the value is ignored.
pub(super) fn matching_post_fact(
    fact: &Fact,
    path: &Path,
    post_bytes: &[u8],
    post_ast: Option<&RustAst>,
    test_assertion_ordinal: Option<usize>,
    commit_sha: &str,
) -> Result<Option<(Span, String)>> {
    match fact {
        Fact::FunctionSignature { qualified_name, .. } => Ok(post_ast.and_then(|ast| {
            function_signature::extract(ast, path).find_map(|f| match f {
                Fact::FunctionSignature {
                    qualified_name: q,
                    span,
                    content_hash,
                    ..
                } if q == *qualified_name => Some((span, content_hash)),
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
        })),
        Fact::Field { qualified_path, .. } => Ok(post_ast.and_then(|ast| {
            field::extract(ast, path).find_map(|f| match f {
                Fact::Field {
                    qualified_path: q,
                    span,
                    content_hash,
                    ..
                } if q == *qualified_path => Some((span, content_hash)),
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
        })),
        Fact::PublicSymbol { qualified_name, .. } => Ok(post_ast.and_then(|ast| {
            // First: check if the item is still bare-pub (happy path).
            let still_bare_pub = symbol_existence::extract(ast, path).find_map(|f| match f {
                Fact::PublicSymbol {
                    qualified_name: q,
                    span,
                    content_hash,
                    ..
                } if q == *qualified_name => Some((span, content_hash)),
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            });
            if still_bare_pub.is_some() {
                return still_bare_pub;
            }
            // Second: item is absent from the bare-pub extract — check whether
            // it still exists with a narrowed visibility (pub(crate), pub(super),
            // pub(in …), or private).  The simple name is the last segment of
            // the qualified_name (e.g. "Config" from "my_mod::Config").
            // `rsplit` always yields at least one element, so `.next()` is `Some`.
            let simple_name = qualified_name
                .rsplit("::")
                .next()
                .expect("rsplit always yields at least one element");
            symbol_existence::find_item_by_name(ast, simple_name).and_then(|found| {
                use symbol_existence::VisibilityKind;
                match found.visibility {
                    // Still bare-pub — would have been caught by the extract above.
                    VisibilityKind::BarePub => None,
                    // Narrowed to restricted or private: signal a structural
                    // change so `classify` emits `StaleSourceChanged`.
                    VisibilityKind::Restricted | VisibilityKind::Private => {
                        Some((found.span, found.content_hash))
                    }
                }
            })
        })),
        Fact::DocClaim {
            qualified_name,
            mention_span,
            mention_hash,
            ..
        } => {
            // Do NOT anchor to the original byte offset: content inserted above
            // the mention shifts its position without changing its text.  Instead,
            // scan the post-commit markdown for all inline-code mentions whose
            // text equals `qualified_name`, then pick the best one via the
            // duplicate-mention tie-breaker.
            let candidates =
                doc_claim::find_mentions(post_bytes, path, commit_sha, qualified_name)?;
            let best = doc_claim::best_mention(&candidates, mention_span, mention_hash);
            Ok(best.map(|m| (m.span.clone(), m.mention_hash.clone())))
        }
        Fact::TestAssertion { test_fn, .. } => Ok(post_ast.and_then(|ast| {
            // Pair T₀ → post by `(test_fn, ordinal)`. `test_assertion::extract`
            // emits one fact per `assert!`/`assert_eq!`/`assert_ne!` invocation
            // in tree-sitter walk order, which matches the ordering used at T₀
            // (see `replay::push_test_assertion_facts`). Returning the
            // post fact at index `ordinal` makes a body-modified assertion at
            // the same position route to `StaleSourceChanged` via
            // `post_hash != observed_hash`, and an unchanged sibling stay
            // `Valid`. An out-of-range ordinal (deleted-tail assertion) yields
            // `None`, which upstream `classify_against_commit` routes via the
            // existing "symbol not found" path — `NeedsRevalidation` when
            // the test fn survives in `commit_index`, `StaleSourceDeleted`
            // otherwise. SPEC §5 rationale in
            // `benchmarks/provbench/spotcheck/2026-05-12-post-pass3-findings.md`.
            let ordinal = test_assertion_ordinal?;
            test_assertion::extract(ast, path, &[])
                .filter_map(|f| match f {
                    Fact::TestAssertion {
                        test_fn: q,
                        span,
                        content_hash,
                        ..
                    } if q == *test_fn => Some((span, content_hash)),
                    Fact::FunctionSignature { .. }
                    | Fact::Field { .. }
                    | Fact::PublicSymbol { .. }
                    | Fact::DocClaim { .. }
                    | Fact::TestAssertion { .. } => None,
                })
                .nth(ordinal)
        })),
    }
}

/// Enumerate all same-kind facts in the post-commit blob for use as rename
/// candidates.  Returns `(qualified_name, span_bytes)` pairs.
pub(super) fn rename_candidates_for(
    fact: &Fact,
    path: &Path,
    post_bytes: &[u8],
    post_ast: Option<&RustAst>,
) -> Vec<(String, Vec<u8>)> {
    let Some(ast) = post_ast else {
        return Vec::new();
    };
    match fact {
        Fact::FunctionSignature { .. } => function_signature::extract(ast, path)
            .filter_map(|f| match f {
                Fact::FunctionSignature {
                    qualified_name,
                    span,
                    ..
                } => Some((qualified_name, post_bytes[span.byte_range].to_vec())),
                Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
            .collect(),
        Fact::Field { .. } => field::extract(ast, path)
            .filter_map(|f| match f {
                Fact::Field {
                    qualified_path,
                    span,
                    ..
                } => Some((qualified_path, post_bytes[span.byte_range].to_vec())),
                Fact::FunctionSignature { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
            .collect(),
        Fact::PublicSymbol { .. } => symbol_existence::extract(ast, path)
            .filter_map(|f| match f {
                Fact::PublicSymbol {
                    qualified_name,
                    span,
                    ..
                } => Some((qualified_name, post_bytes[span.byte_range].to_vec())),
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
            .collect(),
        Fact::DocClaim { .. } => Vec::new(),
        Fact::TestAssertion { .. } => test_assertion::extract(ast, path, &[])
            .filter_map(|f| match f {
                Fact::TestAssertion { test_fn, span, .. } => {
                    Some((test_fn, post_bytes[span.byte_range].to_vec()))
                }
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. } => None,
            })
            .collect(),
    }
}

/// Typed variant of [`rename_candidates_for`].
///
/// Returns [`RenameCandidate`] values whose `container` and `leaf_name` are
/// pre-computed from the qualified name, ready for the typed filter pipeline
/// in [`crate::diff::rename_candidate_typed`].
pub(super) fn rename_candidates_for_typed(
    fact: &Fact,
    path: &Path,
    post_bytes: &[u8],
    post_ast: Option<&RustAst>,
) -> Vec<RenameCandidate> {
    rename_candidates_for(fact, path, post_bytes, post_ast)
        .into_iter()
        .map(|(qname, span)| RenameCandidate::new(qname, span))
        .collect()
}
