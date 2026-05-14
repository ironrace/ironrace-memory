//! R3 symbol_missing — SPEC §5.2.
//! Symbol no longer resolves at the original path AND R7 didn't fire
//! -> Stale (stale_source_deleted).

use super::{Decision, RowCtx, Rule};

pub struct R3SymbolMissing;

impl Rule for R3SymbolMissing {
    fn rule_id(&self) -> &'static str {
        "R3"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §5.2"
    }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        if !matches!(
            ctx.fact.kind.as_str(),
            "FunctionSignature" | "Field" | "PublicSymbol"
        ) {
            return None;
        }
        let (Some(post), _t0) = (ctx.post_blob, ctx.t0_blob) else {
            return None;
        };
        let needle = ctx.fact.symbol_path.as_bytes();
        if needle.is_empty() {
            return None; // empty symbol_path: cannot do substring search, defer to later rules
        }
        let haystack = post;
        // Naive substring search — symbol no longer literally appears.
        let resolves = haystack.windows(needle.len()).any(|w| w == needle);
        if !resolves {
            return Some((
                Decision::Stale,
                format!(
                    r#"{{"rule":"R3","reason":"stale_source_deleted","symbol":"{}"}}"#,
                    ctx.fact.symbol_path
                ),
            ));
        }
        None
    }
}
