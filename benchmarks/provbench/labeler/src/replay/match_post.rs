//! Post-commit fact matching helpers.
//!
//! Given a T₀ fact and the post-commit blob/AST for its source file, these
//! functions locate the corresponding post-commit entry (if any) or enumerate
//! same-kind candidates for rename detection.  They have no dependency on
//! [`super::Replay`], [`super::CommitState`], or `run_inner` — everything
//! they need comes in through their parameters.

use crate::ast::{
    spans::{content_hash, Span},
    RustAst,
};
use crate::diff::RenameCandidate;
use crate::facts::{field, function_signature, symbol_existence, test_assertion, Fact};
use std::path::Path;

/// Search the post-commit blob/AST for the same fact (by kind + qualified
/// key).  Returns `(post_span, post_content_hash)` when found, or `None`
/// when the symbol is absent from this file at the post-commit state.
pub(super) fn matching_post_fact(
    fact: &Fact,
    path: &Path,
    post_bytes: &[u8],
    post_ast: Option<&RustAst>,
) -> Option<(Span, String)> {
    match fact {
        Fact::FunctionSignature { qualified_name, .. } => post_ast.and_then(|ast| {
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
        }),
        Fact::Field { qualified_path, .. } => post_ast.and_then(|ast| {
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
        }),
        Fact::PublicSymbol { qualified_name, .. } => post_ast.and_then(|ast| {
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
        }),
        Fact::DocClaim { mention_span, .. } => {
            let range = mention_span.byte_range.clone();
            if range.end > post_bytes.len() {
                return None;
            }
            Some((
                Span {
                    byte_range: range.clone(),
                    line_start: mention_span.line_start,
                    line_end: mention_span.line_end,
                },
                content_hash(&post_bytes[range]),
            ))
        }
        Fact::TestAssertion { test_fn, .. } => post_ast.and_then(|ast| {
            test_assertion::extract(ast, path, &[]).find_map(|f| match f {
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
        }),
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
