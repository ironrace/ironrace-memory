//! Function-signature fact extractor. Walks a Rust tree-sitter tree and
//! yields Fact::FunctionSignature items with module-qualified names. The
//! signature span covers leading attributes + visibility + `fn NAME(...) -> R`
//! and stops before the function body.

use crate::ast::{line_span_through, spans::content_hash, RustAst};
use crate::facts::Fact;
use std::path::Path;
use tree_sitter::Node;

/// Back-compat shim returning `(qualified_name, signature_span)` pairs.
///
/// Retained only so [`RustAst::function_signature_spans`] (used by
/// pre-extractor tests) keeps compiling. New callers must use
/// [`extract`] directly. The shim feeds `extract` an empty source path
/// and discards the resulting [`Fact`] structure.
#[doc(hidden)]
pub fn iter(ast: &RustAst) -> impl Iterator<Item = (String, crate::ast::spans::Span)> + '_ {
    extract(ast, Path::new(""))
        .filter_map(|f| {
            #[allow(unreachable_patterns)]
            match f {
                Fact::FunctionSignature {
                    qualified_name,
                    span,
                    ..
                } => Some((qualified_name, span)),
                // Keep this arm: when Tasks 6-9 add Fact variants, rustc
                // cannot prove exhaustiveness here until they exist. The
                // allow keeps the arm in place without a lint failure.
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
}

/// Yield one [`Fact::FunctionSignature`] per `fn` declaration in `ast`,
/// tagged with `source_path` for `fact_id` formatting.
///
/// The signature span runs from any leading attributes / doc comments
/// through the end of `fn NAME(...) -> R` and stops before the body
/// brace; trailing whitespace inside that range is trimmed so the span
/// always ends on a non-whitespace byte.
pub fn extract<'a>(ast: &'a RustAst, source_path: &'a Path) -> impl Iterator<Item = Fact> + 'a {
    let mut out = Vec::new();
    let src = ast.source();
    let root = ast.root();
    walk(root, src, &[], source_path, &mut out);
    out.into_iter()
}

fn walk(node: Node<'_>, src: &[u8], mod_path: &[String], source_path: &Path, out: &mut Vec<Fact>) {
    let kind = node.kind();
    if kind == "function_item" {
        if let Some(fact) = extract_one(node, src, mod_path, source_path) {
            out.push(fact);
        }
    }
    if kind == "mod_item" {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(name) = name_node.utf8_text(src) {
                let mut next = mod_path.to_vec();
                next.push(name.to_string());
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    walk(child, src, &next, source_path, out);
                }
                return;
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, mod_path, source_path, out);
    }
}

fn extract_one(
    node: Node<'_>,
    src: &[u8],
    mod_path: &[String],
    source_path: &Path,
) -> Option<Fact> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(src).ok()?.to_string();
    let qualified_name = if mod_path.is_empty() {
        name
    } else {
        let mut s = mod_path.join("::");
        s.push_str("::");
        s.push_str(&name);
        s
    };
    let start_node = leading_attribute_or_self(node);
    let body = node.child_by_field_name("body");
    let raw_end = body
        .map(|b| b.start_byte())
        .unwrap_or_else(|| node.end_byte());
    // Strip trailing whitespace so the signature span ends at the last
    // non-whitespace byte before the body brace (matches the Task 4 stub).
    let sig_end_byte = src[start_node.start_byte()..raw_end]
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|rel| start_node.start_byte() + rel + 1)
        .unwrap_or(raw_end);
    let span = line_span_through(src, start_node, sig_end_byte);
    let hash = content_hash(&src[span.byte_range.clone()]);
    Some(Fact::FunctionSignature {
        qualified_name,
        source_path: source_path.to_path_buf(),
        span,
        content_hash: hash,
    })
}

fn leading_attribute_or_self(node: Node<'_>) -> Node<'_> {
    let mut start = node;
    while let Some(prev) = start.prev_sibling() {
        match prev.kind() {
            "attribute_item" | "inner_attribute_item" | "line_comment" | "block_comment" => {
                start = prev;
            }
            _ => break,
        }
    }
    start
}
