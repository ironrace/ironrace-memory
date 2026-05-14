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
        // SPEC §10 pilot tuning: search for the **leaf** symbol name
        // (last `::`-separated component) rather than the qualified path.
        // Rust source never literally contains `Type::field`; the field is
        // declared inside its parent struct on a separate line.
        let qualified = ctx.fact.symbol_path.as_str();
        let leaf = leaf_symbol(qualified);
        if leaf.is_empty() {
            return None; // defensive: empty/unparseable symbol_path
        }
        let needle = leaf.as_bytes();
        let haystack = post;
        // Naive substring search — symbol no longer literally appears.
        let resolves = haystack.windows(needle.len()).any(|w| w == needle);
        if !resolves {
            return Some((
                Decision::Stale,
                format!(
                    r#"{{"rule":"R3","reason":"stale_source_deleted","symbol":"{}","leaf":"{}"}}"#,
                    qualified, leaf
                ),
            ));
        }
        None
    }
}

/// Extract the leaf segment of a `::`-qualified symbol path (e.g.
/// `Type::method` → `method`, `module::Type::method` → `method`).
/// Returns the input unchanged when no `::` is present.
fn leaf_symbol(qualified: &str) -> &str {
    qualified.rsplit("::").next().unwrap_or(qualified)
}
