//! Per SPEC §5 rule 3: whitespace-only or comment-only diffs do not invalidate
//! a fact even when content hashes differ. Implementation tokenizes both
//! sides with tree-sitter, drops trivia, and compares the residual.

use tree_sitter::{Node, Parser, Tree};

pub fn is_whitespace_or_comment_only(before: &[u8], after: &[u8]) -> bool {
    let parse = |s: &[u8]| -> Option<Tree> {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_rust::LANGUAGE.into()).ok()?;
        p.parse(s, None)
    };
    let Some(b_tree) = parse(before) else {
        return false;
    };
    let Some(a_tree) = parse(after) else {
        return false;
    };
    let mut b_toks: Vec<&[u8]> = Vec::new();
    let mut a_toks: Vec<&[u8]> = Vec::new();
    collect_significant_tokens(b_tree.root_node(), before, &mut b_toks);
    collect_significant_tokens(a_tree.root_node(), after, &mut a_toks);
    b_toks == a_toks
}

fn collect_significant_tokens<'a>(node: Node<'_>, src: &'a [u8], out: &mut Vec<&'a [u8]>) {
    let kind = node.kind();
    if kind == "line_comment" || kind == "block_comment" {
        return;
    }
    if node.child_count() == 0 {
        let s = &src[node.byte_range()];
        if !s.iter().all(|b| b.is_ascii_whitespace()) {
            out.push(s);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_significant_tokens(child, src, out);
    }
}

/// Upper-bound on leaf-name similarity for rename detection.
///
/// When two candidates share a near-identical name (e.g. `replace_with_captures`
/// vs `replace_with_caps`, similarity ≈ 0.89), the high span similarity is
/// driven almost entirely by the near-identical name rather than structural
/// evidence.  Such pairs represent independent sibling symbols, not renames.
/// Candidates whose leaf-name similarity meets or exceeds this bound are
/// excluded from rename detection.
const MAX_NAME_SIMILARITY: f32 = 0.85;

/// Per SPEC §5 rule 2: when a symbol no longer resolves, search post-commit
/// candidates for one whose Myers-diff similarity (via
/// `similar::TextDiff::ratio()`) is ≥ `min_ratio` over symbol-bearing lines.
///
/// Two-part gate (both must pass):
/// 1. **Span similarity** ≥ `min_ratio` — confirms the body/signature is
///    structurally close enough to warrant rename consideration.
/// 2. **Leaf-name similarity** in [`min_ratio`, `MAX_NAME_SIMILARITY`) — the
///    candidate's leaf name must resemble the original (lower bound: some
///    naming relationship exists) but must not be nearly identical (upper
///    bound: prevents sibling symbols whose high span-level ratio is driven
///    entirely by a nearly-unchanged name from being treated as renames).
///
/// Returns the best (highest span-ratio) candidate name above both thresholds,
/// or `None`.
pub fn rename_candidate(
    before_span: &[u8],
    after_candidates: &[(String, Vec<u8>)],
    min_ratio: f32,
) -> Option<String> {
    let before = String::from_utf8_lossy(before_span);
    let before_leaf = extract_leaf_name_from_span(&before);
    let mut best: Option<(String, f32)> = None;
    for (name, span) in after_candidates {
        let after = String::from_utf8_lossy(span);
        let span_ratio = similar::TextDiff::from_chars(before.as_ref(), after.as_ref()).ratio();
        if span_ratio < min_ratio {
            continue;
        }
        // Gate 2: leaf-name similarity must be in [min_ratio, MAX_NAME_SIMILARITY).
        // — Lower bound: the candidate's name must have some resemblance to the
        //   original (rules out completely unrelated symbols that happen to share
        //   a common body pattern).
        // — Upper bound: the candidate's name must differ enough to represent a
        //   genuine rename rather than a coincidental sibling with a nearly-
        //   identical name.
        let candidate_leaf = leaf_name_from_qualified(name);
        let name_ratio = similar::TextDiff::from_chars(before_leaf, candidate_leaf).ratio();
        if name_ratio < min_ratio || name_ratio >= MAX_NAME_SIMILARITY {
            continue;
        }
        match &best {
            None => best = Some((name.clone(), span_ratio)),
            Some((_, r)) if span_ratio > *r => best = Some((name.clone(), span_ratio)),
            _ => {}
        }
    }
    best.map(|(n, _)| n)
}

/// Extract the last `::` segment of a qualified name.
///
/// `"AstAnalysis::any_literal"` → `"any_literal"`
/// `"captures_mut"` → `"captures_mut"`
fn leaf_name_from_qualified(qualified: &str) -> &str {
    qualified.rsplit("::").next().unwrap_or(qualified)
}

/// Best-effort extraction of the primary identifier (leaf name) from a raw
/// span byte slice (as stored by the labeler).
///
/// Handles the two most common forms:
/// - Function declaration: `[pub] fn <name>(` → extracts `<name>`.
/// - Field / other: leading identifier up to the first non-identifier byte.
///
/// Falls back to the full span string when no identifier boundary is found.
fn extract_leaf_name_from_span(span: &str) -> &str {
    // Try `fn <name>` pattern (function signatures).
    if let Some(after_fn) = span.find("fn ") {
        let rest = &span[after_fn + 3..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        return &rest[..end];
    }
    // Fall back: leading identifier (field names, symbol names).
    // Skip leading `pub ` or `pub(…) ` if present.
    let trimmed = span.trim_start();
    let trimmed = trimmed
        .strip_prefix("pub(")
        .and_then(|s| s.find(')').map(|i| s[i + 1..].trim_start()))
        .or_else(|| trimmed.strip_prefix("pub "))
        .unwrap_or(trimmed);
    let end = trimmed
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(trimmed.len());
    &trimmed[..end]
}
