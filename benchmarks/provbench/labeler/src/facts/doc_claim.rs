//! Extracts `Fact::DocClaim` by scanning markdown for inline-code mentions
//! (`` `name` ``) that back-resolve to a known fact's last qualified-name
//! segment (case-sensitive, exact match).
//!
//! Only inline code (`Event::Code`) is handled in v1; fenced code blocks are
//! skipped intentionally.

use std::path::Path;

use pulldown_cmark::{Event, Options, Parser};

use crate::ast::spans::{content_hash, Span};
use crate::facts::Fact;

// ── lookup helpers ───────────────────────────────────────────────────────────

/// Returns `(last_segment, &defining_span, defining_hash)` for fact variants
/// that carry a qualified name, a span, and a content hash.
fn fact_lookup_key(f: &Fact) -> Option<(&str, &Span, &str)> {
    use Fact::*;
    match f {
        FunctionSignature {
            qualified_name,
            span,
            content_hash,
            ..
        } => Some((
            qualified_name.rsplit("::").next()?,
            span,
            content_hash.as_str(),
        )),
        Field {
            qualified_path,
            span,
            content_hash,
            ..
        } => Some((
            qualified_path.rsplit("::").next()?,
            span,
            content_hash.as_str(),
        )),
        PublicSymbol {
            qualified_name,
            span,
            content_hash,
            ..
        } => Some((
            qualified_name.rsplit("::").next()?,
            span,
            content_hash.as_str(),
        )),
        // DocClaim and TestAssertion have no source span to resolve against.
        DocClaim { .. } | TestAssertion { .. } => None,
    }
}

// ── line counting ─────────────────────────────────────────────────────────────

/// Returns the 1-based line number that byte offset `pos` falls on, by
/// counting `\n` bytes in `md[..pos]`.
fn line_at(md: &[u8], pos: usize) -> u32 {
    1 + md[..pos].iter().filter(|&&b| b == b'\n').count() as u32
}

// ── public API ────────────────────────────────────────────────────────────────

/// Scan `md_bytes` for inline-code events that resolve against `known_facts`.
///
/// Returns an iterator of `Fact::DocClaim` values — one per (mention, matching
/// fact) pair.
pub fn extract<'a>(
    md_bytes: &'a [u8],
    doc_path: &'a Path,
    known_facts: &'a [Fact],
) -> impl Iterator<Item = Fact> + 'a {
    // Build lookup table: (last_segment, &span, hash_str).
    let lookup: Vec<(&str, &Span, &str)> = known_facts.iter().filter_map(fact_lookup_key).collect();

    // Convert bytes → str; yield nothing on invalid UTF-8.
    let md_str = std::str::from_utf8(md_bytes).unwrap_or_default();

    Parser::new_ext(md_str, Options::all())
        .into_offset_iter()
        .filter_map(move |(event, range)| {
            // Only inline code (`name`) in v1.
            let text = match &event {
                Event::Code(s) => s.clone(),
                _ => return None,
            };

            // Find the first fact whose last segment matches the inline-code text.
            let (_, def_span, def_hash) =
                lookup.iter().find(|(seg, _, _)| *seg == text.as_ref())?;

            // Compute the mention span from the pulldown-cmark byte range.
            let mention_bytes = &md_bytes[range.clone()];
            let mention_span = Span {
                byte_range: range.clone(),
                line_start: line_at(md_bytes, range.start),
                line_end: line_at(md_bytes, range.end),
            };
            let mention_hash = content_hash(mention_bytes);

            Some(Fact::DocClaim {
                qualified_name: text.into_string(),
                doc_path: doc_path.to_path_buf(),
                mention_span,
                mention_hash,
                defining_span: (*def_span).clone(),
                defining_hash: def_hash.to_string(),
            })
        })
}
