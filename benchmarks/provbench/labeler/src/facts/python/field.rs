//! Python class/field AST walker. Task 5 shipped [`iter_classes`], which
//! yields `(class_name, header_span)` pairs for every `class_definition`
//! node. The header span covers `class NAME[(bases)]:` and stops before the
//! body block.
//!
//! Task 7 adds [`extract`], which walks every `class_definition` body and
//! emits one [`Fact::Field`] per *direct* class-body assignment. Instance
//! attributes (`self.x = ...` inside methods) are NOT class fields and are
//! intentionally excluded — they are owned by `symbol_existence` (Task 8).

use crate::ast::python::PythonAst;
use crate::ast::spans::{content_hash, Span};
use crate::facts::Fact;
use std::path::{Path, PathBuf};
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

/// Yield one [`Fact::Field`] per *direct* class-body assignment. Only
/// `expression_statement > assignment` whose `left` is a single
/// `identifier` qualifies — tuple/destructuring assignments and method-
/// scope `self.x = ...` bindings are intentionally skipped. Walks nested
/// classes (`class A: class B: x = 1` emits `A.B.x`) but never recurses
/// into `function_definition.body`.
pub fn extract<'a>(ast: &'a PythonAst, source_path: &'a Path) -> impl Iterator<Item = Fact> + 'a {
    let mut out = Vec::new();
    let module_path = super::module_path_for(source_path);
    walk_class_fields(
        ast.root(),
        ast.source(),
        &mut Vec::new(),
        &module_path,
        source_path,
        &mut out,
    );
    out.into_iter()
}

fn walk_class_fields(
    node: Node<'_>,
    src: &[u8],
    class_path: &mut Vec<String>,
    module_path: &str,
    source_path: &Path,
    out: &mut Vec<Fact>,
) {
    match node.kind() {
        "class_definition" => {
            let name = match node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(src).ok())
            {
                Some(s) => s.to_string(),
                None => return,
            };
            class_path.push(name);
            if let Some(body) = node.child_by_field_name("body") {
                let mut c = body.walk();
                for child in body.children(&mut c) {
                    // Direct class-body field assignments.
                    if child.kind() == "expression_statement" && !class_path.is_empty() {
                        if let Some(fact) =
                            build_field_fact(child, src, class_path, module_path, source_path)
                        {
                            out.push(fact);
                        }
                    }
                    // Recurse so nested classes are reached, but
                    // walk_class_fields itself skips function bodies.
                    walk_class_fields(child, src, class_path, module_path, source_path, out);
                }
            }
            class_path.pop();
            return;
        }
        "function_definition" => {
            // Do NOT descend into method bodies — `self.x = ...` is
            // an instance attribute, not a class field.
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_class_fields(child, src, class_path, module_path, source_path, out);
    }
}

fn build_field_fact(
    expr_stmt: Node<'_>,
    src: &[u8],
    class_path: &[String],
    module_path: &str,
    source_path: &Path,
) -> Option<Fact> {
    // expression_statement -> assignment (first named child).
    let mut cursor = expr_stmt.walk();
    let assignment = expr_stmt
        .named_children(&mut cursor)
        .find(|c| c.kind() == "assignment")?;
    let left = assignment.child_by_field_name("left")?;
    if left.kind() != "identifier" {
        return None;
    }
    let field_name = left.utf8_text(src).ok()?.to_string();
    let type_text = assignment
        .child_by_field_name("type")
        .and_then(|n| n.utf8_text(src).ok())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let start_byte = expr_stmt.start_byte();
    let end_byte = expr_stmt.end_byte();
    let trimmed_end = src[start_byte..end_byte]
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|rel| start_byte + rel + 1)
        .unwrap_or(end_byte);
    let line_end_row = src[..trimmed_end].iter().filter(|b| **b == b'\n').count() as u32 + 1;
    let span = Span {
        byte_range: start_byte..trimmed_end,
        line_start: (expr_stmt.start_position().row + 1) as u32,
        line_end: line_end_row,
    };
    let hash = content_hash(&src[span.byte_range.clone()]);
    let qualified_path = format!("{module_path}.{}.{field_name}", class_path.join("."));
    Some(Fact::Field {
        qualified_path,
        source_path: PathBuf::from(source_path),
        type_text,
        span,
        content_hash: hash,
    })
}
