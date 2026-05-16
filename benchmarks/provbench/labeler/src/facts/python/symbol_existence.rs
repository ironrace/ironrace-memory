//! Python module-constant AST walker. Task 5 only ships
//! [`iter_module_constants`], which yields `(name, assignment_span)` pairs
//! for every module-scope `expression_statement > assignment` whose LHS is
//! a single identifier matching `^[A-Z][A-Z0-9_]*$` (SCREAMING_SNAKE_CASE).
//! Full PublicSymbol-fact emission lands in Task 8.

use crate::ast::python::PythonAst;
use crate::ast::spans::Span;
use tree_sitter::Node;

pub fn iter_module_constants(ast: &PythonAst) -> impl Iterator<Item = (String, Span)> + '_ {
    let mut out: Vec<(String, Span)> = Vec::new();
    let src = ast.source();
    let root = ast.root();
    // Walk only direct children of the module — module-level assignments only.
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if let Some((name, span)) = extract_one(child, src) {
            out.push((name, span));
        }
    }
    out.into_iter()
}

fn extract_one(stmt: Node<'_>, src: &[u8]) -> Option<(String, Span)> {
    if stmt.kind() != "expression_statement" {
        return None;
    }
    // expression_statement wraps a single child (in our case, the assignment).
    let mut cursor = stmt.walk();
    let inner = stmt.children(&mut cursor).next()?;
    if inner.kind() != "assignment" {
        return None;
    }
    let left = inner.child_by_field_name("left")?;
    if left.kind() != "identifier" {
        return None;
    }
    let name = left.utf8_text(src).ok()?.to_string();
    if !is_screaming_snake_case(&name) {
        return None;
    }
    let start_byte = stmt.start_byte();
    let end_byte = stmt.end_byte();
    let span = Span {
        byte_range: start_byte..end_byte,
        line_start: (stmt.start_position().row + 1) as u32,
        line_end: (stmt.end_position().row + 1) as u32,
    };
    Some((name, span))
}

fn is_screaming_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}
