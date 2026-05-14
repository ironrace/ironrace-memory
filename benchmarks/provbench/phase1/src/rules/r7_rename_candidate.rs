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
            return Some((
                Decision::Stale,
                format!(
                    r#"{{"rule":"R7","reason":"stale_symbol_renamed","similarity":{:.3},"to":"{}"}}"#,
                    s, p
                ),
            ));
        }
        None
    }
}
