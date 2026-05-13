//! Function-signature fact extractor. Walks a Rust tree-sitter tree and
//! yields Fact::FunctionSignature items with module-qualified names. The
//! signature span covers leading attributes + visibility + `fn NAME(...) -> R`
//! and stops before the function body.

use crate::ast::{line_span_through, spans::content_hash, RustAst};
use crate::facts::Fact;
use std::path::Path;
use tree_sitter::Node;

/// Back-compat shim returning `(qualified_name, signature_span)` pairs.
///
/// Retained only so [`RustAst::function_signature_spans`] (used by
/// pre-extractor tests) keeps compiling. New callers must use
/// [`extract`] directly. The shim feeds `extract` an empty source path
/// and discards the resulting [`Fact`] structure.
#[doc(hidden)]
pub fn iter(ast: &RustAst) -> impl Iterator<Item = (String, crate::ast::spans::Span)> + '_ {
    extract(ast, Path::new(""))
        .filter_map(|f| {
            #[allow(unreachable_patterns)]
            match f {
                Fact::FunctionSignature {
                    qualified_name,
                    span,
                    ..
                } => Some((qualified_name, span)),
                // Keep this arm: when Tasks 6-9 add Fact variants, rustc
                // cannot prove exhaustiveness here until they exist. The
                // allow keeps the arm in place without a lint failure.
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
}

/// Yield one [`Fact::FunctionSignature`] per `fn` declaration in `ast`,
/// tagged with `source_path` for `fact_id` formatting.
///
/// The signature span runs from any leading attributes / doc comments
/// through the end of `fn NAME(...) -> R` and stops before the body
/// brace; trailing whitespace inside that range is trimmed so the span
/// always ends on a non-whitespace byte.
pub fn extract<'a>(ast: &'a RustAst, source_path: &'a Path) -> impl Iterator<Item = Fact> + 'a {
    extract_observations(ast, source_path).map(|o| o.fact)
}

/// Private replay-time observation: the public [`Fact`] plus structural
/// disambiguators (`cfg_attribute_set`, `impl_receiver_type`) used by
/// the replay matcher to pair T₀ → post when the same `qualified_name`
/// appears in multiple cfg-gated or multi-impl definitions in the
/// same file.
///
/// The disambiguators are NOT serialized into [`Fact::FunctionSignature`]
/// or `fact_id`; the labeler's corpus schema is byte-stable across
/// pass-5. Only the private replay layer reads them.
#[derive(Debug, Clone)]
pub(crate) struct FunctionSignatureObservation {
    /// The public fact emitted by `extract`. Always a
    /// [`Fact::FunctionSignature`] variant.
    pub fact: Fact,
    /// Normalized `#[cfg(...)]` / `#[cfg_attr(...)]` attribute texts
    /// attached to the function (leading attribute siblings), sorted
    /// and deduped so the set is order-independent.
    pub cfg_attribute_set: Vec<String>,
    /// Text of the enclosing `impl <T> { … }` receiver type, or
    /// `None` for module-level functions. For `impl <Trait> for <Type>`
    /// returns the receiver type (`<Type>`).
    pub impl_receiver_type: Option<String>,
}

/// Same as [`extract`] but additionally returns the cfg-attribute set
/// and enclosing impl-receiver type for each function, packaged as a
/// [`FunctionSignatureObservation`].
///
/// Used by the replay layer to build the private `FnDisambiguator`
/// disambiguator. The emitted [`Fact`] values and their order are
/// identical to what `extract` produces — `extract` is now a thin
/// mapper over this function, so T₀ extraction output is byte-stable.
pub(crate) fn extract_observations<'a>(
    ast: &'a RustAst,
    source_path: &'a Path,
) -> impl Iterator<Item = FunctionSignatureObservation> + 'a {
    let mut out = Vec::new();
    let src = ast.source();
    let root = ast.root();
    walk_with_context(root, src, &[], None, source_path, &mut out);
    out.into_iter()
}

fn walk_with_context(
    node: Node<'_>,
    src: &[u8],
    mod_path: &[String],
    impl_receiver: Option<&str>,
    source_path: &Path,
    out: &mut Vec<FunctionSignatureObservation>,
) {
    let kind = node.kind();
    if kind == "function_item" {
        if let Some(fact) = extract_one(node, src, mod_path, source_path) {
            out.push(FunctionSignatureObservation {
                fact,
                cfg_attribute_set: collect_cfg_attributes(node, src),
                impl_receiver_type: impl_receiver.map(|s| s.to_string()),
            });
        }
    }
    if kind == "mod_item" {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(name) = name_node.utf8_text(src) {
                let mut next = mod_path.to_vec();
                next.push(name.to_string());
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    // Entering a `mod` resets the impl-receiver
                    // context: a nested fn inside `mod inner { ... }`
                    // is module-level relative to `inner`, not inside
                    // the outer impl block.
                    walk_with_context(child, src, &next, None, source_path, out);
                }
                return;
            }
        }
    }
    if kind == "impl_item" {
        let receiver = impl_receiver_type_text(node, src);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk_with_context(child, src, mod_path, receiver.as_deref(), source_path, out);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_with_context(child, src, mod_path, impl_receiver, source_path, out);
    }
}

/// Collect the normalized text of every `#[cfg(...)]` / `#[cfg_attr(...)]`
/// attribute attached as a leading sibling to `fn_node` (via the same
/// pattern used by `leading_attribute_or_self` for span computation).
/// Returns a sorted, deduplicated vec so the result is order-independent.
fn collect_cfg_attributes(fn_node: Node<'_>, src: &[u8]) -> Vec<String> {
    let mut cfgs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut cursor = fn_node;
    while let Some(prev) = cursor.prev_sibling() {
        match prev.kind() {
            "attribute_item" | "inner_attribute_item" => {
                if let Ok(text) = prev.utf8_text(src) {
                    let trimmed = text.trim();
                    if is_cfg_attribute_text(trimmed) {
                        cfgs.insert(normalize_attribute_text(trimmed));
                    }
                }
                cursor = prev;
            }
            "line_comment" | "block_comment" => {
                cursor = prev;
            }
            _ => break,
        }
    }
    cfgs.into_iter().collect()
}

/// `true` when `text` is an attribute item whose first identifier is
/// `cfg` or `cfg_attr`. We do a string check rather than parsing into
/// the tree-sitter attribute subtree because that subtree's shape is
/// not perfectly stable across tree-sitter-rust versions; the raw
/// attribute text is.
fn is_cfg_attribute_text(text: &str) -> bool {
    // Attribute items look like `#[cfg(...)]` or `#[cfg_attr(...)]`,
    // possibly preceded by `#!` for inner attributes (which we ignore
    // for cfg gates on functions). Strip leading `#[` / `#![` and
    // check the first identifier.
    let body = text
        .strip_prefix("#[")
        .or_else(|| text.strip_prefix("#!["))
        .unwrap_or(text);
    let ident: String = body
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    ident == "cfg" || ident == "cfg_attr"
}

/// Normalize an attribute text for set comparison: collapse runs of
/// ASCII whitespace to a single space. We deliberately preserve all
/// other characters (quotes, escapes, parentheses) so two attributes
/// that differ only in formatting compare equal.
fn normalize_attribute_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_ws = false;
    for ch in text.chars() {
        if ch.is_ascii_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out.trim().to_string()
}

/// For an `impl <T> { … }` or `impl <Trait> for <Type> { … }` node,
/// return the text of the receiver type (`<T>` or `<Type>` respectively).
/// Returns `None` if the impl has no `type` field child (shouldn't
/// happen on well-formed code, but we are defensive).
fn impl_receiver_type_text(impl_node: Node<'_>, src: &[u8]) -> Option<String> {
    let ty_node = impl_node.child_by_field_name("type")?;
    ty_node.utf8_text(src).ok().map(|s| s.trim().to_string())
}

fn extract_one(
    node: Node<'_>,
    src: &[u8],
    mod_path: &[String],
    source_path: &Path,
) -> Option<Fact> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(src).ok()?.to_string();
    let qualified_name = if mod_path.is_empty() {
        name
    } else {
        let mut s = mod_path.join("::");
        s.push_str("::");
        s.push_str(&name);
        s
    };
    let start_node = leading_attribute_or_self(node);
    let body = node.child_by_field_name("body");
    let raw_end = body
        .map(|b| b.start_byte())
        .unwrap_or_else(|| node.end_byte());
    // Strip trailing whitespace so the signature span ends at the last
    // non-whitespace byte before the body brace (matches the Task 4 stub).
    let sig_end_byte = src[start_node.start_byte()..raw_end]
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|rel| start_node.start_byte() + rel + 1)
        .unwrap_or(raw_end);
    let span = line_span_through(src, start_node, sig_end_byte);
    let hash = content_hash(&src[span.byte_range.clone()]);
    Some(Fact::FunctionSignature {
        qualified_name,
        source_path: source_path.to_path_buf(),
        span,
        content_hash: hash,
    })
}

fn leading_attribute_or_self(node: Node<'_>) -> Node<'_> {
    let mut start = node;
    while let Some(prev) = start.prev_sibling() {
        match prev.kind() {
            "attribute_item" | "inner_attribute_item" | "line_comment" | "block_comment" => {
                start = prev;
            }
            _ => break,
        }
    }
    start
}
