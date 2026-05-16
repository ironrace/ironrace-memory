//! Python function-signature AST walker. Yields `(leaf_name, signature_span)`
//! for every `function_definition` node (both module-level and class methods,
//! sync and `async`). The signature span runs from the start of the
//! `function_definition` node (covering `def`/`async def`, name, parameters,
//! and optional return type) and stops before the body block — i.e. the
//! trailing `:` is included, the body indentation is not.
//!
//! Task 5 shipped the leaf-name [`iter`] used by [`crate::ast::python::PythonAst`]
//! tests. Task 6 adds [`extract`] which emits full [`Fact::FunctionSignature`]
//! rows with dotted module-qualified names (e.g. `src.example.Greeter.greet`).
//! The two walkers are intentionally kept separate so `iter`'s leaf-name
//! contract stays byte-stable for the AST-layer tests.

use crate::ast::python::PythonAst;
use crate::ast::spans::{content_hash, Span};
use crate::facts::Fact;
use std::path::{Path, PathBuf};
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

/// Yield one [`Fact::FunctionSignature`] per `def` / `async def`,
/// qualified with the module path derived from `source_path` and any
/// enclosing class. Signature span follows the same boundary as [`iter`]:
/// from the start of the def keyword through (but excluding) the body,
/// with trailing whitespace trimmed so the span ends on the `:`.
///
/// Nested defs (functions defined inside other functions) are NOT
/// emitted — they are addressed by `symbol_existence` (Task 8).
pub fn extract<'a>(ast: &'a PythonAst, source_path: &'a Path) -> impl Iterator<Item = Fact> + 'a {
    let mut out = Vec::new();
    let module_path = super::module_path_for(source_path);
    walk_qualified(
        ast.root(),
        ast.source(),
        &mut Vec::new(),
        &module_path,
        source_path,
        &mut out,
    );
    out.into_iter()
}

fn walk_qualified(
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
                    walk_qualified(child, src, class_path, module_path, source_path, out);
                }
            }
            class_path.pop();
            return;
        }
        "function_definition" => {
            if let Some(fact) = build_function_fact(node, src, class_path, module_path, source_path)
            {
                out.push(fact);
            }
            // Do NOT descend into the body — nested defs are owned by
            // symbol_existence (Task 8), not function_signature.
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_qualified(child, src, class_path, module_path, source_path, out);
    }
}

fn build_function_fact(
    node: Node<'_>,
    src: &[u8],
    class_path: &[String],
    module_path: &str,
    source_path: &Path,
) -> Option<Fact> {
    let (leaf, span) = extract_one(node, src)?;
    let qualified_name = if class_path.is_empty() {
        format!("{module_path}.{leaf}")
    } else {
        format!("{module_path}.{}.{leaf}", class_path.join("."))
    };
    let hash = content_hash(&src[span.byte_range.clone()]);
    Some(Fact::FunctionSignature {
        qualified_name,
        source_path: PathBuf::from(source_path),
        span,
        content_hash: hash,
    })
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
