//! Function-signature fact extractor. Walks a Rust tree-sitter tree and
//! yields (qualified-or-bare name, signature-span) pairs. The signature
//! span ends immediately before the function body — body changes alone
//! are NOT a signature change.

use crate::ast::{line_span_through, spans::Span, RustAst};
use tree_sitter::Node;

pub fn iter(ast: &RustAst) -> impl Iterator<Item = (String, Span)> + '_ {
    let mut out = Vec::new();
    let src = ast.source();
    let root = ast.root();
    walk(root, src, &mut out);
    out.into_iter()
}

fn walk(node: Node<'_>, src: &[u8], out: &mut Vec<(String, Span)>) {
    if node.kind() == "function_item" {
        if let Some((name, span)) = extract_signature(node, src) {
            out.push((name, span));
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, out);
    }
}

fn extract_signature(node: Node<'_>, src: &[u8]) -> Option<(String, Span)> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(src).ok()?.to_string();
    let body = node.child_by_field_name("body");
    let raw_end = body
        .map(|b| b.start_byte())
        .unwrap_or_else(|| node.end_byte());
    // Strip trailing ASCII whitespace so the span ends at the last
    // non-whitespace byte before the body brace (or end of node).
    let sig_end_byte = src[node.start_byte()..raw_end]
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|rel| node.start_byte() + rel + 1)
        .unwrap_or(raw_end);
    let span = line_span_through(src, node, sig_end_byte);
    Some((name, span))
}
