//! Python module-binding AST walker. Two entry points share this module:
//!
//! - [`iter_module_constants`] (Task 5) yields `(name, assignment_span)`
//!   pairs for module-scope `expression_statement > assignment` whose LHS
//!   is a single identifier matching `^[A-Z][A-Z0-9_]*$`
//!   (SCREAMING_SNAKE_CASE). This is the byte-stable contract that
//!   [`crate::ast::python::PythonAst`] tests depend on — do not change it.
//! - [`extract`] (Task 8) yields full [`Fact::PublicSymbol`] rows for
//!   every direct module-level binding: `def`/`async def` (header
//!   only), `class` (header only), and `expression_statement >
//!   assignment` with a single-identifier LHS (no case filter — the
//!   full statement is the bound span).
//!
//! The walker is intentionally non-recursive: nested defs / class
//! attributes / methods are owned by other extractors
//! (`function_signature`, `field`).

use crate::ast::python::PythonAst;
use crate::ast::spans::{content_hash, Span};
use crate::facts::Fact;
use std::path::{Path, PathBuf};
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

/// Yield one [`Fact::PublicSymbol`] per direct module-level binding.
///
/// Walks only the direct children of the `module` root — does NOT
/// recurse. Three child kinds emit a fact:
///
/// - `function_definition` (covers `def` and `async def`; in
///   tree-sitter-python 0.25 `async def` parses as a
///   `function_definition` with a leading `async` keyword child)
///   → header span = start of node through `body.start_byte()`,
///   trailing whitespace trimmed so the span ends on the `:`.
/// - `class_definition` → same header-only convention.
/// - `expression_statement` whose single child is an `assignment`
///   with a single-identifier LHS → span = the entire
///   `expression_statement` (header == statement).
///
/// `qualified_name = "{module_path}.{leaf_name}"`, with `module_path`
/// derived from `source_path` via [`super::module_path_for`].
/// `content_hash` is the SHA-256 of the bound span's bytes.
pub fn extract<'a>(ast: &'a PythonAst, source_path: &'a Path) -> impl Iterator<Item = Fact> + 'a {
    let module_path = super::module_path_for(source_path);
    let mut out: Vec<Fact> = Vec::new();
    let src = ast.source();
    let root = ast.root();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                if let Some(fact) = build_header_fact(child, src, &module_path, source_path) {
                    out.push(fact);
                }
            }
            "class_definition" => {
                if let Some(fact) = build_header_fact(child, src, &module_path, source_path) {
                    out.push(fact);
                }
            }
            "expression_statement" => {
                if let Some(fact) = build_assignment_fact(child, src, &module_path, source_path) {
                    out.push(fact);
                }
            }
            _ => {}
        }
    }
    out.into_iter()
}

/// Build a header-only fact for `function_definition` / `class_definition`:
/// span runs from the node start through (but not including) the body
/// block, with trailing whitespace trimmed so the span ends on `:`.
fn build_header_fact(
    node: Node<'_>,
    src: &[u8],
    module_path: &str,
    source_path: &Path,
) -> Option<Fact> {
    let name_node = node.child_by_field_name("name")?;
    let leaf = name_node.utf8_text(src).ok()?.to_string();
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
    let hash = content_hash(&src[span.byte_range.clone()]);
    Some(Fact::PublicSymbol {
        qualified_name: format!("{module_path}.{leaf}"),
        source_path: PathBuf::from(source_path),
        span,
        content_hash: hash,
    })
}

/// Build a fact for a module-level `expression_statement > assignment`
/// whose LHS is a single identifier. Span covers the whole
/// `expression_statement` (no body / header distinction for assignments).
fn build_assignment_fact(
    stmt: Node<'_>,
    src: &[u8],
    module_path: &str,
    source_path: &Path,
) -> Option<Fact> {
    let mut cursor = stmt.walk();
    let inner = stmt.children(&mut cursor).next()?;
    if inner.kind() != "assignment" {
        return None;
    }
    let left = inner.child_by_field_name("left")?;
    if left.kind() != "identifier" {
        return None;
    }
    let leaf = left.utf8_text(src).ok()?.to_string();
    let start_byte = stmt.start_byte();
    let end_byte = stmt.end_byte();
    let span = Span {
        byte_range: start_byte..end_byte,
        line_start: (stmt.start_position().row + 1) as u32,
        line_end: (stmt.end_position().row + 1) as u32,
    };
    let hash = content_hash(&src[span.byte_range.clone()]);
    Some(Fact::PublicSymbol {
        qualified_name: format!("{module_path}.{leaf}"),
        source_path: PathBuf::from(source_path),
        span,
        content_hash: hash,
    })
}
