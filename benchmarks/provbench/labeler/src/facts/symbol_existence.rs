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
    extract_occurrences(ast, source_path).map(|o| o.fact)
}

/// Private replay-time form tag distinguishing the kind of public-
/// surface declaration that emitted a `Fact::PublicSymbol`.
///
/// Pass-5 Cluster G uses this to recognize `pub use` re-exports
/// (`BarePubUse`) as "still-public continuity" even when the post
/// declaration span hashes differently than a T₀ definition span:
/// the public name `X` is still exported from the crate even though
/// the underlying form changed from `pub fn X` to `pub use … X`.
///
/// `Definition` covers `pub fn`, `pub struct`, `pub enum`, `pub mod`,
/// `pub trait`, `pub const`, `pub static`, `pub type`.
/// `BarePubUse` covers `pub use …` (including `pub use … as X`).
/// Restricted-visibility uses (`pub(crate) use`, `pub(super) use`,
/// `pub(in …) use`, plain `use`) are NOT emitted by
/// `extract_use_declaration` (gated by `is_bare_pub`), so they don't
/// appear as occurrences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicSymbolForm {
    /// `pub fn X / pub struct X / pub enum X / pub mod X / pub trait X
    /// / pub const X / pub static X / pub type X`.
    Definition,
    /// `pub use path::X;`, `pub use path::{X, …};`, or
    /// `pub use path::Old as X;`. Glob re-exports
    /// (`pub use path::*;`) are NOT represented here in pass-5 —
    /// they remain out of scope and fall through to existing absent-
    /// symbol logic.
    BarePubUse {
        /// The source-side path text up to (but not including) the
        /// imported leaf. For `pub use a::b::c;` this is `"a::b"`;
        /// for `pub use a::Old as c;` this is `"a"`. May be empty
        /// for bare `pub use X;`.
        source_path: String,
        /// `Some(original_name)` for `pub use … Original as Alias;`.
        /// `None` when the re-export keeps the original identifier.
        alias: Option<String>,
    },
}

/// `Fact::PublicSymbol` plus the structural form-tag describing
/// which extraction branch emitted it.
#[derive(Debug, Clone)]
pub struct PublicSymbolOccurrence {
    pub fact: Fact,
    pub form: PublicSymbolForm,
}

/// Same as [`extract`] but additionally returns each fact's structural
/// form tag, packaged as a [`PublicSymbolOccurrence`].
///
/// Used by the replay matcher to distinguish bare `pub use` re-exports
/// (which preserve the exported name's public-surface continuity even
/// when the declaration form changed) from direct `pub <kind>`
/// definitions. The emitted [`Fact`] values and their order are
/// identical to what [`extract`] produces, so T₀ extraction is byte-
/// stable across pass-5.
pub fn extract_occurrences<'a>(
    ast: &'a RustAst,
    source_path: &'a Path,
) -> impl Iterator<Item = PublicSymbolOccurrence> + 'a {
    let mut out = Vec::new();
    let src = ast.source();
    let root = ast.root();
    walk_with_form(root, src, source_path, &mut out);
    out.into_iter()
}

fn walk_with_form(
    node: Node<'_>,
    src: &[u8],
    source_path: &Path,
    out: &mut Vec<PublicSymbolOccurrence>,
) {
    match node.kind() {
        "function_item" | "struct_item" | "enum_item" | "mod_item" | "trait_item"
        | "const_item" | "static_item" | "type_item" => {
            let pre_len = out.len();
            extract_named_item_occurrences(node, src, source_path, out);
            // post-condition: each newly pushed occurrence is Definition
            for occ in &mut out[pre_len..] {
                occ.form = PublicSymbolForm::Definition;
            }
            // Still recurse into mod_item bodies to catch nested pub items.
            if node.kind() == "mod_item" {
                if let Some(body) = node.child_by_field_name("body") {
                    let mut cursor = body.walk();
                    for child in body.named_children(&mut cursor) {
                        walk_with_form(child, src, source_path, out);
                    }
                }
            }
            return;
        }
        "use_declaration" => {
            extract_use_declaration_occurrences(node, src, source_path, out);
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_with_form(child, src, source_path, out);
    }
}

fn extract_named_item_occurrences(
    node: Node<'_>,
    src: &[u8],
    source_path: &Path,
    out: &mut Vec<PublicSymbolOccurrence>,
) {
    let mut tmp = Vec::new();
    extract_named_item(node, src, source_path, &mut tmp);
    for f in tmp {
        out.push(PublicSymbolOccurrence {
            fact: f,
            form: PublicSymbolForm::Definition,
        });
    }
}

fn extract_use_declaration_occurrences(
    node: Node<'_>,
    src: &[u8],
    source_path: &Path,
    out: &mut Vec<PublicSymbolOccurrence>,
) {
    let vis = match visibility_child(node) {
        Some(v) => v,
        None => return,
    };
    if !is_bare_pub(vis, src) {
        return;
    }

    // Find the argument subtree (everything after the visibility modifier).
    let mut cursor = node.walk();
    let arg = node
        .named_children(&mut cursor)
        .find(|c| c.kind() != "visibility_modifier");
    let arg = match arg {
        Some(a) => a,
        None => return,
    };

    let span = line_span_from_node(src, node);
    let hash = content_hash(&src[span.byte_range.clone()]);

    // Collect (exported_leaf, form_details) for each name introduced.
    let leaves = collect_use_leaves(arg, src);
    for leaf in leaves {
        let form = PublicSymbolForm::BarePubUse {
            source_path: leaf.source_path,
            alias: leaf.alias,
        };
        let fact = Fact::PublicSymbol {
            qualified_name: leaf.exported_name,
            source_path: source_path.to_path_buf(),
            span: span.clone(),
            content_hash: hash.clone(),
        };
        out.push(PublicSymbolOccurrence { fact, form });
    }
}

/// Per-leaf decomposition of a `pub use` argument tree. The
/// `exported_name` is what appears in the crate's public surface (the
/// alias when present, else the original leaf identifier).
#[derive(Debug, Clone)]
struct UseLeaf {
    exported_name: String,
    source_path: String,
    alias: Option<String>,
}

/// Walk a `pub use` argument subtree and yield one [`UseLeaf`] per
/// exported name. Mirrors the recursion shape of [`collect_use_names`]
/// but threads the path-prefix and alias text alongside the leaf.
fn collect_use_leaves(node: Node<'_>, src: &[u8]) -> Vec<UseLeaf> {
    let mut out = Vec::new();
    collect_use_leaves_inner(node, src, "", &mut out);
    out
}

fn collect_use_leaves_inner(node: Node<'_>, src: &[u8], prefix: &str, out: &mut Vec<UseLeaf>) {
    match node.kind() {
        "identifier" => {
            if let Ok(name) = node.utf8_text(src) {
                out.push(UseLeaf {
                    exported_name: name.to_string(),
                    source_path: prefix.to_string(),
                    alias: None,
                });
            }
        }
        "scoped_identifier" => {
            // Last identifier child = exported leaf.
            let mut cursor = node.walk();
            let idents: Vec<Node<'_>> = node
                .named_children(&mut cursor)
                .filter(|c| c.kind() == "identifier")
                .collect();
            if let Some(last) = idents.last() {
                if let Ok(name) = last.utf8_text(src) {
                    let path_parts: Vec<String> = idents[..idents.len().saturating_sub(1)]
                        .iter()
                        .filter_map(|n| n.utf8_text(src).ok().map(|s| s.to_string()))
                        .collect();
                    let mut source_path = prefix.to_string();
                    if !source_path.is_empty() && !path_parts.is_empty() {
                        source_path.push_str("::");
                    }
                    source_path.push_str(&path_parts.join("::"));
                    out.push(UseLeaf {
                        exported_name: name.to_string(),
                        source_path,
                        alias: None,
                    });
                }
            }
        }
        "use_as_clause" => {
            // The alias identifier is the LAST child; the original
            // path may be the first scoped_identifier / identifier.
            let mut cursor = node.walk();
            let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
            // Find the alias (rightmost identifier).
            let alias_node = children
                .iter()
                .rev()
                .find(|c| c.kind() == "identifier")
                .copied();
            // Find the path (leftmost identifier or scoped_identifier).
            let path_node = children
                .iter()
                .find(|c| matches!(c.kind(), "identifier" | "scoped_identifier"))
                .copied();
            if let (Some(alias_n), Some(path_n)) = (alias_node, path_node) {
                if let Ok(alias) = alias_n.utf8_text(src) {
                    // Derive the original-name + path from the path node.
                    let (original_name, source_path) = decompose_use_path(path_n, src, prefix);
                    if let Some(original_name) = original_name {
                        out.push(UseLeaf {
                            exported_name: alias.to_string(),
                            source_path,
                            alias: Some(original_name),
                        });
                    }
                }
            }
        }
        "scoped_use_list" => {
            // path::{list}. Build a new prefix from the path part,
            // then recurse into the use_list.
            let mut cursor = node.walk();
            let mut new_prefix = prefix.to_string();
            for child in node.named_children(&mut cursor) {
                match child.kind() {
                    "scoped_identifier" | "identifier" => {
                        if let Ok(text) = child.utf8_text(src) {
                            if !new_prefix.is_empty() {
                                new_prefix.push_str("::");
                            }
                            new_prefix.push_str(text);
                        }
                    }
                    "use_list" => {
                        collect_use_leaves_inner(child, src, &new_prefix, out);
                    }
                    _ => {}
                }
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_use_leaves_inner(child, src, prefix, out);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_use_leaves_inner(child, src, prefix, out);
            }
        }
    }
}

/// Decompose the path-node of a `use_as_clause` into `(original_leaf,
/// source_path)`. The leaf is the last identifier; the path is the
/// prefix joined with the rest of the scoped identifier.
fn decompose_use_path(path_node: Node<'_>, src: &[u8], prefix: &str) -> (Option<String>, String) {
    if path_node.kind() == "identifier" {
        return (
            path_node.utf8_text(src).ok().map(|s| s.to_string()),
            prefix.to_string(),
        );
    }
    if path_node.kind() == "scoped_identifier" {
        let mut cursor = path_node.walk();
        let idents: Vec<Node<'_>> = path_node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "identifier")
            .collect();
        if let Some(last) = idents.last() {
            let name = last.utf8_text(src).ok().map(|s| s.to_string());
            let path_parts: Vec<String> = idents[..idents.len().saturating_sub(1)]
                .iter()
                .filter_map(|n| n.utf8_text(src).ok().map(|s| s.to_string()))
                .collect();
            let mut source_path = prefix.to_string();
            if !source_path.is_empty() && !path_parts.is_empty() {
                source_path.push_str("::");
            }
            source_path.push_str(&path_parts.join("::"));
            return (name, source_path);
        }
    }
    (None, prefix.to_string())
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
#[derive(Debug)]
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
        // Terminal named-item kinds. We early-return None after checking the
        // item itself because these kinds don't contain sibling named items
        // accessible by simple name (a `fn` body has locals, not exports).
        // Only `mod_item` recurses, since modules contain nested items.
        // If you add a new item kind here that DOES contain reachable named
        // items (e.g. `impl_item`), remove the early return for that arm.
        "function_item" | "struct_item" | "enum_item" | "mod_item" | "trait_item"
        | "const_item" | "static_item" | "type_item" | "union_item" => {
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

    let vis_node = visibility_child(node);
    let vis_kind = match vis_node {
        Some(v) if is_bare_pub(v, src) => VisibilityKind::BarePub,
        Some(_) => VisibilityKind::Restricted,
        None => VisibilityKind::Private,
    };

    // Build span from the first token of the item through the name's end byte.
    // Use `line_span_from_node` on the full item node so we capture everything
    // the way `extract_named_item` does for bare-pub items.
    let start_node = vis_node.unwrap_or(name_node);
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
