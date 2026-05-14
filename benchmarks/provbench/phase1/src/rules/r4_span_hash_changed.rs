//! R4 span_lines_present — SPEC §5.3 (Phase 1 line-presence variant).
//!
//! SPEC §10 pilot tuning: the labeler's `content_hash_at_observation` is
//! computed over a sub-line byte range that phase1 cannot reproduce from
//! `line_span` alone (the byte range comes from tree-sitter span
//! extraction at labeling time, but only the line numbers leak into the
//! emitted fact JSON). Phase 1 instead checks whether the **T0 source
//! lines that bound the fact** still appear verbatim somewhere in the
//! post-commit file:
//!
//! 1. Extract `t0_span` = T0 `lines[start..=end]` (the fact's bound
//!    lines exactly).
//! 2. If `t0_span` contains the leaf symbol name (defensive: trivial
//!    lines like `}` or `#[test]` would otherwise collapse every fact
//!    onto the same probe) AND `t0_span` is a substring of post_blob
//!    → **Valid** (the fact's bound source is byte-stable; line
//!    numbers may have shifted but the code didn't).
//! 3. Otherwise → **Stale** (the lines that bound the fact have
//!    meaningfully changed).
//!
//! Rationale: this trades the labeler-internal `byte_range`+hash check
//! for a byte-window presence check that phase1 can compute from
//! line_span alone. Valid rows with intra-file line shifts (the bulk
//! of canary GT=Valid rows) are caught by the substring match. The
//! leaf-symbol guard prevents trivial single-token lines from masking
//! genuine staleness.

use super::{Decision, RowCtx, Rule};

/// Minimum non-whitespace probe length below which we refuse to use the
/// T0 line slice as a uniqueness probe. Lines this short are
/// dominated by syntax noise (`}`, `{`, `;`, `*/`) and produce too many
/// false-positive matches.
const MIN_PROBE_NONWS_LEN: usize = 8;

/// Stricter length guard used for `TestAssertion` facts: the leaf
/// identifier is the *containing test fn* name, not anything on the
/// asserted line, so we lean on length alone to avoid matching
/// trivial assertions like `assert!(x);`.
const MIN_PROBE_NONWS_LEN_ASSERTION: usize = 20;

pub struct R4SpanHashChanged;

impl Rule for R4SpanHashChanged {
    fn rule_id(&self) -> &'static str {
        "R4"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §5.3"
    }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        let (Some(post), Some(t0)) = (ctx.post_blob, ctx.t0_blob) else {
            return None;
        };
        let line_start = ctx.fact.line_span[0];
        let line_end = ctx.fact.line_span[1];
        let t0_span = extract_lines(t0, [line_start, line_end]);

        // Out-of-bounds span → no signal; fall through.
        if t0_span.is_empty() {
            return None;
        }

        // Leaf-symbol guard (kind-conditional):
        //   - For symbol-bearing kinds (FunctionSignature/Field/
        //     PublicSymbol), the leaf identifier IS the line — drop
        //     the guard if it's already in the probe.
        //   - For TestAssertion the leaf is the *containing test fn*
        //     name; the fact's line is the assertion body itself,
        //     which doesn't mention the test fn — so the leaf guard
        //     would over-reject. Use a stricter length guard instead.
        let leaf = leaf_symbol(&ctx.fact.symbol_path);
        let probe_has_leaf =
            !leaf.is_empty() && t0_span.windows(leaf.len()).any(|w| w == leaf.as_bytes());

        // Min-probe-length guard: too-short probes (after stripping
        // whitespace) are ambiguous noise.
        let nonws_len = t0_span.iter().filter(|b| !b.is_ascii_whitespace()).count();

        let guard_passed = match ctx.fact.kind.as_str() {
            "TestAssertion" => nonws_len >= MIN_PROBE_NONWS_LEN_ASSERTION,
            _ => probe_has_leaf && nonws_len >= MIN_PROBE_NONWS_LEN,
        };

        if guard_passed && contains_subslice(post, &t0_span) {
            return Some((
                Decision::Valid,
                serde_json::json!({
                    "rule": "R4",
                    "reason": "t0_span_found_in_post",
                })
                .to_string(),
            ));
        }

        // Probe absent (or below guard) → the source lines that bound
        // this fact at T0 have meaningfully changed, OR phase1 cannot
        // distinguish stale from valid here. Route to Stale to preserve
        // §7.1 stale recall — false positives against GT=Valid are
        // contained by the leaf+length guards above.
        Some((
            Decision::Stale,
            serde_json::json!({
                "rule": "R4",
                "reason": "stale_source_changed",
            })
            .to_string(),
        ))
    }
}

fn leaf_symbol(qualified: &str) -> &str {
    qualified.rsplit("::").next().unwrap_or(qualified)
}

fn extract_lines(src: &[u8], span: [u64; 2]) -> Vec<u8> {
    let mut out = Vec::new();
    let (start, end) = (span[0] as usize, span[1] as usize);
    if start == 0 || end < start {
        return out;
    }
    for (i, line) in src.split(|&b| b == b'\n').enumerate() {
        let lineno = i + 1;
        if lineno >= start && lineno <= end {
            out.extend_from_slice(line);
            out.push(b'\n');
        }
        if lineno > end {
            break;
        }
    }
    out
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}
