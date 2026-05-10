//! Extracts `Fact::DocClaim` by scanning markdown for inline-code mentions
//! (`` `name` ``) that back-resolve to a known fact's last qualified-name
//! segment (case-sensitive, exact match).
//!
//! Only inline code (`Event::Code`) is handled in v1; fenced code blocks are
//! skipped intentionally.

use std::path::Path;

use anyhow::{Context, Result};
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
/// Returns the `Fact::DocClaim` values — one per (mention, matching fact)
/// pair. Returns an `Err` if `md_bytes` is not valid UTF-8: silently producing
/// zero facts on a corrupted README is an undetectable determinism failure.
pub fn extract(md_bytes: &[u8], doc_path: &Path, known_facts: &[Fact]) -> Result<Vec<Fact>> {
    // Build lookup table: (last_segment, &span, hash_str).
    let lookup: Vec<(&str, &Span, &str)> = known_facts.iter().filter_map(fact_lookup_key).collect();

    // Convert bytes → str; surface invalid UTF-8 as an error so reviewers can
    // locate the offending blob.
    let md_str = std::str::from_utf8(md_bytes)
        .with_context(|| format!("invalid UTF-8 in markdown at {}", doc_path.display()))?;

    let facts = Parser::new_ext(md_str, Options::all())
        .into_offset_iter()
        .filter_map(|(event, range)| {
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
        .collect();

    Ok(facts)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::RustAst;
    use crate::facts::symbol_existence;

    /// `[0xC3, 0x28]` is a known-invalid 2-byte UTF-8 sequence: `0xC3` starts
    /// a 2-byte codepoint but `0x28` is not a valid continuation byte.
    #[test]
    fn invalid_utf8_returns_err_with_path_context() {
        let bytes = [0xC3, 0x28];
        let path = Path::new("docs/CORRUPT.md");
        let err = extract(&bytes, path, &[]).expect_err("invalid UTF-8 must surface as Err");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("docs/CORRUPT.md"),
            "error must include doc path, got: {msg}"
        );
        assert!(
            msg.to_lowercase().contains("utf"),
            "error must mention UTF-8, got: {msg}"
        );
    }

    #[test]
    #[allow(invalid_from_utf8)]
    fn from_utf8_actually_rejects_chosen_bytes() {
        // Sanity: confirm the bytes we feed to the test really are invalid.
        // The compiler also detects this — `#[allow]` keeps clippy happy.
        assert!(std::str::from_utf8(&[0xC3, 0x28]).is_err());
    }

    #[test]
    fn valid_utf8_still_produces_facts() {
        let rs = b"pub fn search() {}\n";
        let md = b"Use `search` to scan.\n";
        let ast = RustAst::parse(rs).unwrap();
        let known: Vec<Fact> = symbol_existence::extract(&ast, Path::new("lib.rs")).collect();
        let claims = extract(md, Path::new("README.md"), &known).expect("valid UTF-8 ok");
        assert_eq!(claims.len(), 1);
    }
}
