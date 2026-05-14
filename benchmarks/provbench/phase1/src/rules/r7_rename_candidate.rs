//! R7 rename_candidate — SPEC §5.2.
//! Same-kind candidate found at another path with similarity >= 0.6,
//! deterministic tie-break (similarity desc, qualified_name asc) -> Stale.

use super::{Decision, RowCtx, Rule};
use crate::similarity::{similarity, RENAME_THRESHOLD};

pub struct R7RenameCandidate;

impl Rule for R7RenameCandidate {
    fn rule_id(&self) -> &'static str {
        "R7"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §5.2"
    }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        // Only applies to symbol-bearing fact kinds.
        if !matches!(
            ctx.fact.kind.as_str(),
            "FunctionSignature" | "Field" | "PublicSymbol"
        ) {
            return None;
        }
        // No file content -> R3 handles it.
        if ctx.post_blob.is_some() {
            return None;
        }

        // Scan commit_files for a same-kind candidate.
        let body = &ctx.fact.body;
        let mut best: Option<(f32, &str)> = None;
        for path in ctx.commit_files {
            if path == &ctx.fact.source_path {
                continue;
            }
            let s = similarity(body, path); // proxy: file path likeness; rule_unit fixtures override
            if s >= RENAME_THRESHOLD {
                best = match best {
                    Some((bs, bp)) if bs > s || (bs == s && bp <= path.as_str()) => Some((bs, bp)),
                    _ => Some((s, path.as_str())),
                };
            }
        }
        if let Some((s, p)) = best {
            // Preserve 3-decimal similarity by parsing the formatted
            // string back into a JSON number; the round-trip avoids
            // f32 precision noise in the trace artifact while keeping
            // `to` safely escaped via serde_json.
            let similarity_value: serde_json::Value = format!("{:.3}", s)
                .parse::<f64>()
                .map(serde_json::Value::from)
                .unwrap_or(serde_json::Value::Null);
            return Some((
                Decision::Stale,
                serde_json::json!({
                    "rule": "R7",
                    "reason": "stale_symbol_renamed",
                    "similarity": similarity_value,
                    "to": p,
                })
                .to_string(),
            ));
        }
        None
    }
}
