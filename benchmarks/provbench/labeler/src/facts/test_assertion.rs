//! Test-assertion fact extractor. Detects `assert!`, `assert_eq!`, and
//! `assert_ne!` macro invocations inside `#[test]`-annotated functions and
//! emits `Fact::TestAssertion` for each one.
//!
//! ## Grammar notes (tree-sitter-rust 0.24)
//!
//! `attribute_item` children: `#`, `[`, `attribute` (which has an `identifier`
//! child), `]`. There is no named `path` field on the attribute — the
//! identifier is reached via the `attribute` named child.
//!
//! `macro_invocation` children: `identifier` (the macro name — NOT via a
//! `macro` field), `!`, `token_tree` (argument list).  Field names for the
//! macro path were not present in the 0.24 grammar; we walk named children
//! directly.

use crate::ast::{line_span_from_node, spans::content_hash, RustAst};
use crate::facts::Fact;
use std::path::Path;
use tree_sitter::Node;

const ASSERT_MACROS: &[&str] = &["assert", "assert_eq", "assert_ne"];

/// Extract `Fact::TestAssertion` items from `ast`.
///
/// `known_facts` is used to resolve the first identifier in each assertion's
/// argument list to a known symbol.  Only `FunctionSignature` and
/// `PublicSymbol` facts carry a `qualified_name`; the last segment of that
/// name is used for matching.
pub fn extract<'a>(
    ast: &'a RustAst,
    source_path: &'a Path,
    known_facts: &'a [Fact],
) -> impl Iterator<Item = Fact> + 'a {
    let src = ast.source();
    let root = ast.root();
    let mut out = Vec::new();
    walk(root, src, source_path, known_facts, &mut out);
    out.into_iter()
}

// ── tree walk ────────────────────────────────────────────────────────────────

fn walk(node: Node<'_>, src: &[u8], source_path: &Path, known_facts: &[Fact], out: &mut Vec<Fact>) {
    if node.kind() == "function_item" {
        if is_test_function(node, src) {
            collect_assertions(node, src, source_path, known_facts, out);
        }
        // Do not recurse into nested functions — they are handled by their
        // own `function_item` node in the outer walk.
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, source_path, known_facts, out);
    }
}

/// Return `true` when `node` (a `function_item`) is preceded by a `#[test]`
/// attribute among its consecutive leading siblings.
fn is_test_function(node: Node<'_>, src: &[u8]) -> bool {
    let mut cursor = node;
    while let Some(prev) = cursor.prev_sibling() {
        match prev.kind() {
            "line_comment" | "block_comment" => {
                cursor = prev;
                continue;
            }
            "attribute_item" => {
                if attribute_item_is_test(prev, src) {
                    return true;
                }
                cursor = prev;
            }
            _ => break,
        }
    }
    false
}

/// Return `true` when `node` is an `attribute_item` whose attribute identifier
/// is exactly `test` (i.e. it represents `#[test]`).
fn attribute_item_is_test(node: Node<'_>, src: &[u8]) -> bool {
    // attribute_item → attribute → identifier
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute" {
            // The first named child of `attribute` is the identifier/path.
            let mut ac = child.walk();
            for attr_child in child.children(&mut ac) {
                if attr_child.kind() == "identifier" {
                    return attr_child.utf8_text(src).ok() == Some("test");
                }
            }
        }
    }
    false
}

/// Walk the body of a `#[test]` function and emit one `Fact::TestAssertion`
/// per `assert!` / `assert_eq!` / `assert_ne!` macro invocation.
fn collect_assertions(
    fn_node: Node<'_>,
    src: &[u8],
    source_path: &Path,
    known_facts: &[Fact],
    out: &mut Vec<Fact>,
) {
    let test_fn_name = match fn_node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())
    {
        Some(name) => name.to_string(),
        None => return,
    };

    let body = match fn_node.child_by_field_name("body") {
        Some(b) => b,
        None => return,
    };

    collect_assertions_in(body, src, &test_fn_name, source_path, known_facts, out);
}

fn collect_assertions_in(
    node: Node<'_>,
    src: &[u8],
    test_fn: &str,
    source_path: &Path,
    known_facts: &[Fact],
    out: &mut Vec<Fact>,
) {
    if node.kind() == "macro_invocation" {
        if let Some(macro_name) = macro_invocation_name(node, src) {
            if ASSERT_MACROS.contains(&macro_name.as_str()) {
                let span = line_span_from_node(src, node);
                let hash = content_hash(&src[span.byte_range.clone()]);
                let asserted_symbol = find_asserted_symbol(node, src, known_facts);
                out.push(Fact::TestAssertion {
                    test_fn: test_fn.to_string(),
                    source_path: source_path.to_path_buf(),
                    span,
                    content_hash: hash,
                    asserted_symbol,
                });
                return; // do not descend into this macro's token tree further
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_assertions_in(child, src, test_fn, source_path, known_facts, out);
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Return the macro name from a `macro_invocation` node.
///
/// In tree-sitter-rust 0.24 the macro path is an `identifier` (or
/// `scoped_identifier`) direct child of `macro_invocation` — NOT accessed via
/// a named field called `macro`.  We pick the first `identifier` child.
fn macro_invocation_name(node: Node<'_>, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            return child.utf8_text(src).ok().map(str::to_string);
        }
    }
    None
}

/// Walk the `token_tree` argument of the `macro_invocation` and return the
/// name of the first top-level `identifier` whose text matches a known fact's
/// last-segment qualified name (case-sensitive exact match).
fn find_asserted_symbol(macro_node: Node<'_>, src: &[u8], known_facts: &[Fact]) -> Option<String> {
    // Collect the set of known last-segment names for O(1) lookup.
    let known_names: std::collections::HashSet<&str> = known_facts
        .iter()
        .filter_map(|f| qualified_name_last_segment(f))
        .collect();

    // Find the `token_tree` child (the macro argument list).
    let token_tree = {
        let mut cursor = macro_node.walk();
        let mut tt = None;
        for child in macro_node.children(&mut cursor) {
            if child.kind() == "token_tree" {
                tt = Some(child);
                break;
            }
        }
        tt?
    };

    // Walk only the IMMEDIATE children of the top-level token_tree for
    // identifiers — do not recurse so that nested calls don't shadow.
    let mut cursor = token_tree.walk();
    for child in token_tree.children(&mut cursor) {
        if child.kind() == "identifier" {
            if let Ok(text) = child.utf8_text(src) {
                if known_names.contains(text) {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

/// Return the last path segment of a fact's `qualified_name`, if it has one.
fn qualified_name_last_segment(fact: &Fact) -> Option<&str> {
    let qn = match fact {
        Fact::FunctionSignature { qualified_name, .. } => qualified_name.as_str(),
        Fact::PublicSymbol { qualified_name, .. } => qualified_name.as_str(),
        _ => return None,
    };
    Some(qn.rsplit("::").next().unwrap_or(qn))
}
