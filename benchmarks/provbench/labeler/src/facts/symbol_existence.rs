//! Public-symbol existence fact extractor. Emits one `Fact::PublicSymbol` per
//! `pub` item (fn, struct, enum, mod, trait, const, static, type) and per name
//! introduced by a `pub use` re-export.
//!
//! Only bare `pub` visibility is matched. `pub(crate)`, `pub(super)`, and
//! `pub(in path)` restricted visibilities are skipped.

use crate::ast::{line_span_from_node, spans::content_hash, RustAst};
use crate::facts::Fact;
use std::path::Path;
use tree_sitter::Node;

/// Extract all `Fact::PublicSymbol` facts from the given AST.
pub fn extract<'a>(ast: &'a RustAst, source_path: &'a Path) -> impl Iterator<Item = Fact> + 'a {
    let mut out = Vec::new();
    let src = ast.source();
    let root = ast.root();
    walk(root, src, source_path, &mut out);
    out.into_iter()
}

fn walk(node: Node<'_>, src: &[u8], source_path: &Path, out: &mut Vec<Fact>) {
    match node.kind() {
        "function_item" | "struct_item" | "enum_item" | "mod_item" | "trait_item"
        | "const_item" | "static_item" | "type_item" => {
            extract_named_item(node, src, source_path, out);
            // Still recurse into mod_item bodies to catch nested pub items.
            if node.kind() == "mod_item" {
                if let Some(body) = node.child_by_field_name("body") {
                    let mut cursor = body.walk();
                    for child in body.named_children(&mut cursor) {
                        walk(child, src, source_path, out);
                    }
                }
            }
            return;
        }
        "use_declaration" => {
            extract_use_declaration(node, src, source_path, out);
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(child, src, source_path, out);
    }
}

/// Return `true` if `node` is a `visibility_modifier` whose text is exactly
/// `"pub"` (bare pub — no parenthesised restriction).
fn is_bare_pub(node: Node<'_>, src: &[u8]) -> bool {
    if node.kind() != "visibility_modifier" {
        return false;
    }
    // Restricted visibility (pub(crate), pub(super), pub(in ...)) has named
    // children inside the visibility_modifier; bare `pub` has none.
    if node.named_child_count() > 0 {
        return false;
    }
    node.utf8_text(src)
        .map(|t| t.trim() == "pub")
        .unwrap_or(false)
}

/// Find the first named child of `node` that is a `visibility_modifier`.
fn visibility_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let result = node
        .named_children(&mut cursor)
        .find(|c| c.kind() == "visibility_modifier");
    result
}

/// Extract a `Fact::PublicSymbol` from a named item (fn, struct, enum, …).
fn extract_named_item(node: Node<'_>, src: &[u8], source_path: &Path, out: &mut Vec<Fact>) {
    let vis = match visibility_child(node) {
        Some(v) => v,
        None => return,
    };
    if !is_bare_pub(vis, src) {
        return;
    }

    // The name field is called "name" for all supported item kinds.
    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };
    let name = match name_node.utf8_text(src) {
        Ok(n) => n.to_string(),
        Err(_) => return,
    };

    // Span: from the visibility modifier's start through the name node's end.
    let span = crate::ast::line_span_through(src, vis, name_node.end_byte());
    let hash = content_hash(&src[span.byte_range.clone()]);

    out.push(Fact::PublicSymbol {
        qualified_name: name,
        source_path: source_path.to_path_buf(),
        span,
        content_hash: hash,
    });
}

/// Extract `Fact::PublicSymbol` facts from a `use_declaration` node.
///
/// Grammar shapes observed (tree-sitter-rust 0.24):
/// - `pub use a::b;`         → use_declaration > scoped_identifier (last ident = "b")
/// - `pub use a::{x, y};`    → use_declaration > scoped_use_list > use_list > identifiers
/// - `pub use a::b as c;`    → use_declaration > use_as_clause > identifier (last = "c")
/// - `pub use a;`            → use_declaration > identifier (bare name)
fn extract_use_declaration(node: Node<'_>, src: &[u8], source_path: &Path, out: &mut Vec<Fact>) {
    let vis = match visibility_child(node) {
        Some(v) => v,
        None => return,
    };
    if !is_bare_pub(vis, src) {
        return;
    }

    // The argument subtree is everything after the visibility modifier.
    // Find it by walking named children and skipping the visibility_modifier.
    let mut cursor = node.walk();
    let arg = node
        .named_children(&mut cursor)
        .find(|c| c.kind() != "visibility_modifier");
    let arg = match arg {
        Some(a) => a,
        None => return,
    };

    // Collect names from the use argument tree.
    let names = collect_use_names(arg, src);
    for name in names {
        // Span: full use_declaration (visibility through semicolon).
        let span = line_span_from_node(src, node);
        let hash = content_hash(&src[span.byte_range.clone()]);
        out.push(Fact::PublicSymbol {
            qualified_name: name,
            source_path: source_path.to_path_buf(),
            span: span.clone(),
            content_hash: hash,
        });
    }
}

/// Recursively collect the exported names from a use-path subtree.
///
/// - `identifier`         → single name (bare `use foo;`)
/// - `scoped_identifier`  → last identifier child
/// - `use_as_clause`      → last identifier (the alias)
/// - `scoped_use_list`    → recurse into the `use_list` (or identifier) part
/// - `use_list`           → recurse over every child
/// - `use_wildcard`       → skip (glob re-exports are not tracked per the v1 plan)
fn collect_use_names(node: Node<'_>, src: &[u8]) -> Vec<String> {
    match node.kind() {
        "identifier" => {
            if let Ok(text) = node.utf8_text(src) {
                vec![text.to_string()]
            } else {
                vec![]
            }
        }
        "scoped_identifier" => {
            // Last named child is the leaf identifier.
            last_identifier(node, src)
                .map(|n| vec![n])
                .unwrap_or_default()
        }
        "use_as_clause" => {
            // The alias (last identifier) is the exported name.
            last_identifier(node, src)
                .map(|n| vec![n])
                .unwrap_or_default()
        }
        "scoped_use_list" => {
            // Walk named children: the use_list (or identifier) comes last.
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .flat_map(|child| collect_use_names(child, src))
                .collect()
        }
        "use_list" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .flat_map(|child| collect_use_names(child, src))
                .collect()
        }
        "use_wildcard" => vec![], // glob re-exports skipped in v1
        _ => vec![],
    }
}

/// Return the text of the last `identifier` named child of `node`.
fn last_identifier(node: Node<'_>, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|c| c.kind() == "identifier" || c.kind() == "type_identifier")
        .last()
        .and_then(|n| n.utf8_text(src).ok())
        .map(|t| t.to_string())
}

// ── Visibility-aware item lookup ──────────────────────────────────────────────

/// Describes the visibility of a Rust item as parsed from its AST node.
///
/// Used by [`find_item_by_name`] to distinguish items that exist but are no
/// longer bare-`pub` from items that are genuinely absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibilityKind {
    /// Bare `pub` — the item is publicly exported.
    BarePub,
    /// Restricted: `pub(crate)`, `pub(super)`, or `pub(in <path>)`.
    Restricted,
    /// No visibility modifier — item is private to its module.
    Private,
}

/// Result of a visibility-aware item lookup.
pub struct FoundItem {
    /// How the item is currently visible.
    pub visibility: VisibilityKind,
    /// Span of the item node (from the visibility modifier / `fn`/`struct`/…
    /// keyword through the end of the name node).
    pub span: crate::ast::spans::Span,
    /// SHA-256 content hash of the span bytes.
    pub content_hash: String,
}

/// Search `ast` for the first named item or `use`-introduced name that matches
/// `simple_name`, regardless of its visibility.
///
/// Returns `None` only when no item with that name exists in the file at all.
/// When the item exists, `FoundItem::visibility` tells you whether it is still
/// bare `pub`, was narrowed to a restricted visibility, or became private.
///
/// Used by the `matching_post_fact` path in `replay.rs` to detect visibility
/// narrowing and emit `StaleSourceChanged` instead of `StaleSourceDeleted`.
pub fn find_item_by_name(ast: &RustAst, simple_name: &str) -> Option<FoundItem> {
    let src = ast.source();
    let root = ast.root();
    find_in_subtree(root, src, simple_name)
}

fn find_in_subtree(node: Node<'_>, src: &[u8], target: &str) -> Option<FoundItem> {
    match node.kind() {
        "function_item" | "struct_item" | "enum_item" | "mod_item" | "trait_item"
        | "const_item" | "static_item" | "type_item" => {
            if let Some(found) = check_named_item(node, src, target) {
                return Some(found);
            }
            // Recurse into mod_item bodies for nested items.
            if node.kind() == "mod_item" {
                if let Some(body) = node.child_by_field_name("body") {
                    let mut cursor = body.walk();
                    for child in body.named_children(&mut cursor) {
                        if let Some(found) = find_in_subtree(child, src, target) {
                            return Some(found);
                        }
                    }
                }
            }
            return None;
        }
        "use_declaration" => {
            return check_use_declaration(node, src, target);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_in_subtree(child, src, target) {
            return Some(found);
        }
    }
    None
}

fn check_named_item(node: Node<'_>, src: &[u8], target: &str) -> Option<FoundItem> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(src).ok()?;
    if name != target {
        return None;
    }

    let vis_kind = match visibility_child(node) {
        Some(v) if is_bare_pub(v, src) => VisibilityKind::BarePub,
        Some(_) => VisibilityKind::Restricted,
        None => VisibilityKind::Private,
    };

    // Build span from the first token of the item through the name's end byte.
    // Use `line_span_from_node` on the full item node so we capture everything
    // the way `extract_named_item` does for bare-pub items.
    let start_node = visibility_child(node).unwrap_or(name_node);
    let span = crate::ast::line_span_through(src, start_node, name_node.end_byte());
    let hash = crate::ast::spans::content_hash(&src[span.byte_range.clone()]);

    Some(FoundItem {
        visibility: vis_kind,
        span,
        content_hash: hash,
    })
}

fn check_use_declaration(node: Node<'_>, src: &[u8], target: &str) -> Option<FoundItem> {
    // Walk named children skipping the visibility_modifier to find the use arg.
    let mut cursor = node.walk();
    let arg = node
        .named_children(&mut cursor)
        .find(|c| c.kind() != "visibility_modifier")?;

    let names = collect_use_names(arg, src);
    if !names.iter().any(|n| n == target) {
        return None;
    }

    let vis_kind = match visibility_child(node) {
        Some(v) if is_bare_pub(v, src) => VisibilityKind::BarePub,
        Some(_) => VisibilityKind::Restricted,
        None => VisibilityKind::Private,
    };

    let span = crate::ast::line_span_from_node(src, node);
    let hash = crate::ast::spans::content_hash(&src[span.byte_range.clone()]);

    Some(FoundItem {
        visibility: vis_kind,
        span,
        content_hash: hash,
    })
}
