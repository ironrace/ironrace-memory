//! Python function-signature AST walker. Yields `(leaf_name, signature_span)`
//! for every `function_definition` node (both module-level and class methods,
//! sync and `async`). The signature span runs from the start of the
//! `function_definition` node (covering `def`/`async def`, name, parameters,
//! and optional return type) and stops before the body block — i.e. the
//! trailing `:` is included, the body indentation is not.
//!
//! Task 5 only ships the leaf name. The full dotted-form
//! `qualified_name` (e.g. `src.example.Greeter.greet`) lands in Task 6.

use crate::ast::python::PythonAst;
use crate::ast::spans::Span;
use tree_sitter::Node;

pub fn iter(ast: &PythonAst) -> impl Iterator<Item = (String, Span)> + '_ {
    let mut out: Vec<(String, Span)> = Vec::new();
    let src = ast.source();
    walk(ast.root(), src, &mut out);
    out.into_iter()
}

fn walk(node: Node<'_>, src: &[u8], out: &mut Vec<(String, Span)>) {
    if node.kind() == "function_definition" {
        if let Some((name, span)) = extract_one(node, src) {
            out.push((name, span));
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, out);
    }
}

fn extract_one(node: Node<'_>, src: &[u8]) -> Option<(String, Span)> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(src).ok()?.to_string();
    let body = node.child_by_field_name("body")?;
    let start_byte = node.start_byte();
    let end_byte = body.start_byte();
    // Trim trailing whitespace so the signature span ends on the `:`.
    let trimmed_end = src[start_byte..end_byte]
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|rel| start_byte + rel + 1)
        .unwrap_or(end_byte);
    let line_end_row = src[..trimmed_end].iter().filter(|b| **b == b'\n').count() as u32 + 1;
    let span = Span {
        byte_range: start_byte..trimmed_end,
        line_start: (node.start_position().row + 1) as u32,
        line_end: line_end_row,
    };
    Some((name, span))
}
