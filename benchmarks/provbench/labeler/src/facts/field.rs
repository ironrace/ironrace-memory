//! Struct/enum field fact extractor. Emits one `Fact::Field` per named field
//! declaration. Tuple-style variant fields and unit variants are skipped.
//!
//! Qualified-path format:
//! - `Foo::field_name`          for struct fields
//! - `E::VariantName::field`    for struct-style enum variant fields

use crate::ast::{line_span_from_node, spans::content_hash, RustAst};
use crate::facts::Fact;
use std::path::Path;
use tree_sitter::Node;

/// Extract all field facts from the given AST, tagging each with `source_path`.
pub fn extract<'a>(ast: &'a RustAst, source_path: &'a Path) -> impl Iterator<Item = Fact> + 'a {
    let mut out = Vec::new();
    let src = ast.source();
    let root = ast.root();
    walk(root, src, source_path, &mut out);
    out.into_iter()
}

/// `true` when the post-commit `ast` emits at least one `Fact::Field`
/// whose leaf (last `::`-segment of `qualified_path`) matches the T₀
/// fact's leaf but whose full `qualified_path` differs.
///
/// Used by `replay::classify_against_commit` as the pass-5 Cluster F
/// file-local routing signal: when a T₀ field's exact qualified path
/// no longer resolves in the post AST but the same leaf name appears
/// in another struct or enum variant in the same file, classify
/// `NeedsRevalidation` (gray area for LLM follow-up). Cross-file
/// field-leaf tracking is intentionally out of scope; this helper
/// only consults the per-file post AST.
pub(crate) fn same_file_leaf_elsewhere(
    ast: &RustAst,
    path: &Path,
    t0_qualified_path: &str,
) -> bool {
    let t0_leaf = match t0_qualified_path.rsplit("::").next() {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    extract(ast, path).any(|f| match f {
        Fact::Field { qualified_path, .. } => {
            qualified_path != t0_qualified_path
                && qualified_path
                    .rsplit("::")
                    .next()
                    .map(|leaf| leaf == t0_leaf)
                    .unwrap_or(false)
        }
        _ => false,
    })
}

fn walk(node: Node<'_>, src: &[u8], source_path: &Path, out: &mut Vec<Fact>) {
    match node.kind() {
        "struct_item" => {
            extract_struct(node, src, source_path, out);
            // Do not recurse further into the struct body — field extractor
            // only needs top-level items per call site.
            return;
        }
        "enum_item" => {
            extract_enum(node, src, source_path, out);
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, source_path, out);
    }
}

fn extract_struct(node: Node<'_>, src: &[u8], source_path: &Path, out: &mut Vec<Fact>) {
    let type_name = match node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())
    {
        Some(n) => n.to_string(),
        None => return,
    };
    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return,
    };
    if body.kind() != "field_declaration_list" {
        return;
    }
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if child.kind() == "field_declaration" {
            if let Some(fact) = extract_field_declaration(child, src, &type_name, None, source_path)
            {
                out.push(fact);
            }
        }
    }
}

fn extract_enum(node: Node<'_>, src: &[u8], source_path: &Path, out: &mut Vec<Fact>) {
    let enum_name = match node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())
    {
        Some(n) => n.to_string(),
        None => return,
    };
    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return,
    };
    // body is enum_variant_list
    let mut cursor = body.walk();
    for variant in body.named_children(&mut cursor) {
        if variant.kind() != "enum_variant" {
            continue;
        }
        let variant_name = match variant
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(src).ok())
        {
            Some(n) => n.to_string(),
            None => continue,
        };
        let variant_body = match variant.child_by_field_name("body") {
            Some(b) => b,
            None => continue, // unit variant — skip
        };
        // Only struct-style variants have field_declaration_list;
        // tuple-style variants have ordered_field_declaration_list — skip.
        if variant_body.kind() != "field_declaration_list" {
            continue;
        }
        let mut vcursor = variant_body.walk();
        for child in variant_body.named_children(&mut vcursor) {
            if child.kind() == "field_declaration" {
                if let Some(fact) = extract_field_declaration(
                    child,
                    src,
                    &enum_name,
                    Some(&variant_name),
                    source_path,
                ) {
                    out.push(fact);
                }
            }
        }
    }
}

/// Build a `Fact::Field` from a single `field_declaration` node.
///
/// `parent` is the struct/enum name; `variant` is `Some(variant_name)` for
/// enum struct-style variants and `None` for plain struct fields.
fn extract_field_declaration(
    node: Node<'_>,
    src: &[u8],
    parent: &str,
    variant: Option<&str>,
    source_path: &Path,
) -> Option<Fact> {
    let field_name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())?
        .to_string();
    let type_node = node.child_by_field_name("type")?;
    let type_text = type_node.utf8_text(src).ok()?.trim().to_string();

    let qualified_path = match variant {
        Some(v) => format!("{parent}::{v}::{field_name}"),
        None => format!("{parent}::{field_name}"),
    };

    let span = line_span_from_node(src, node);
    let hash = content_hash(&src[span.byte_range.clone()]);

    Some(Fact::Field {
        qualified_path,
        source_path: source_path.to_path_buf(),
        type_text,
        span,
        content_hash: hash,
    })
}
