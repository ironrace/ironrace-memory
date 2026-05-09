//! Per SPEC §5.3: whitespace-only or comment-only diffs do not invalidate
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

/// Per SPEC §5.2: when a symbol no longer resolves, search post-commit
/// candidates for one whose Myers-diff similarity (via
/// `similar::TextDiff::ratio()`) is ≥ `min_ratio` over symbol-bearing lines.
/// Returns the best (highest-ratio) candidate name above the threshold, or
/// `None`.
pub fn rename_candidate(
    before_span: &[u8],
    after_candidates: &[(String, Vec<u8>)],
    min_ratio: f32,
) -> Option<String> {
    let before = String::from_utf8_lossy(before_span);
    let mut best: Option<(String, f32)> = None;
    for (name, span) in after_candidates {
        let after = String::from_utf8_lossy(span);
        let ratio = similar::TextDiff::from_chars(before.as_ref(), after.as_ref()).ratio();
        if ratio >= min_ratio {
            match &best {
                None => best = Some((name.clone(), ratio)),
                Some((_, r)) if ratio > *r => best = Some((name.clone(), ratio)),
                _ => {}
            }
        }
    }
    best.map(|(n, _)| n)
}
