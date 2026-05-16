//! Python class/field AST walker. Task 5 only ships [`iter_classes`], which
//! yields `(class_name, header_span)` pairs for every `class_definition`
//! node. The header span covers `class NAME[(bases)]:` and stops before the
//! body block. Full Field-fact emission lands in Task 7.

use crate::ast::python::PythonAst;
use crate::ast::spans::Span;
use tree_sitter::Node;

pub fn iter_classes(ast: &PythonAst) -> impl Iterator<Item = (String, Span)> + '_ {
    let mut out: Vec<(String, Span)> = Vec::new();
    let src = ast.source();
    walk(ast.root(), src, &mut out);
    out.into_iter()
}

fn walk(node: Node<'_>, src: &[u8], out: &mut Vec<(String, Span)>) {
    if node.kind() == "class_definition" {
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
