//! Python test-assertion extractor. Emits one [`Fact::TestAssertion`] per
//! `assert` statement (and unittest-style `self.assert*(...)` call) found
//! inside a test function. Test functions are detected by:
//!
//! 1. Module-scope `def test_*` — `test_fn` is the bare function name.
//! 2. Methods named `test_*` inside a class whose name starts with `Test`
//!    — `test_fn` is `"ClassName.method_name"`.
//!
//! Non-test functions, helper methods, and methods on non-`Test*` classes
//! are skipped. The span covers the full `assert ...` statement (or the
//! `self.assertX(...)` call), with trailing whitespace trimmed so the span
//! ends on the last non-whitespace byte. `asserted_symbol` is always
//! `None` for Python; Task 11's resolver can populate it if needed.

use crate::ast::python::PythonAst;
use crate::ast::spans::{content_hash, Span};
use crate::facts::Fact;
use std::path::{Path, PathBuf};
use tree_sitter::Node;

/// Yield one [`Fact::TestAssertion`] per `assert` / unittest-assert found
/// inside a top-level `def test_*` or inside a `def test_*` method of a
/// `class Test*`.
pub fn extract<'a>(ast: &'a PythonAst, source_path: &'a Path) -> impl Iterator<Item = Fact> + 'a {
    let mut out = Vec::new();
    let src = ast.source();
    let root = ast.root();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                if let Some(name) = name_of(child, src) {
                    if name.starts_with("test_") {
                        walk_for_asserts(child, src, &name, source_path, &mut out);
                    }
                }
            }
            "class_definition" => {
                let cls = match name_of(child, src) {
                    Some(n) => n,
                    None => continue,
                };
                if !cls.starts_with("Test") {
                    continue;
                }
                if let Some(body) = child.child_by_field_name("body") {
                    let mut cc = body.walk();
                    for class_child in body.children(&mut cc) {
                        if class_child.kind() != "function_definition" {
                            continue;
                        }
                        let mname = match name_of(class_child, src) {
                            Some(n) => n,
                            None => continue,
                        };
                        if !mname.starts_with("test_") {
                            continue;
                        }
                        let qual = format!("{cls}.{mname}");
                        walk_for_asserts(class_child, src, &qual, source_path, &mut out);
                    }
                }
            }
            _ => {}
        }
    }
    out.into_iter()
}

fn name_of(node: Node<'_>, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())
        .map(|s| s.to_string())
}

fn walk_for_asserts(
    fn_node: Node<'_>,
    src: &[u8],
    test_fn: &str,
    source_path: &Path,
    out: &mut Vec<Fact>,
) {
    if let Some(body) = fn_node.child_by_field_name("body") {
        descend(body, src, test_fn, source_path, out);
    }
}

fn descend(node: Node<'_>, src: &[u8], test_fn: &str, source_path: &Path, out: &mut Vec<Fact>) {
    if node.kind() == "assert_statement" {
        if let Some(fact) = build_fact(node, src, test_fn, source_path) {
            out.push(fact);
        }
        return;
    }
    if is_unittest_assert_call(node, src) {
        if let Some(fact) = build_fact(node, src, test_fn, source_path) {
            out.push(fact);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        descend(child, src, test_fn, source_path, out);
    }
}

/// Detect `self.assertXxx(...)` (and any `obj.assertXxx(...)`) call
/// expressions. We match on shape only: `call > function: attribute`
/// where the attribute's last segment begins with the literal `assert`.
fn is_unittest_assert_call(node: Node<'_>, src: &[u8]) -> bool {
    if node.kind() != "call" {
        return false;
    }
    let func = match node.child_by_field_name("function") {
        Some(n) => n,
        None => return false,
    };
    if func.kind() != "attribute" {
        return false;
    }
    let attr = match func.child_by_field_name("attribute") {
        Some(n) => n,
        None => return false,
    };
    if attr.kind() != "identifier" {
        return false;
    }
    match attr.utf8_text(src) {
        Ok(s) => s.starts_with("assert"),
        Err(_) => false,
    }
}

fn build_fact(node: Node<'_>, src: &[u8], test_fn: &str, source_path: &Path) -> Option<Fact> {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();
    if end_byte <= start_byte {
        return None;
    }
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
    Some(Fact::TestAssertion {
        test_fn: test_fn.to_string(),
        source_path: PathBuf::from(source_path),
        span,
        content_hash: hash,
        asserted_symbol: None,
    })
}
