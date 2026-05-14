//! R6 doc_claim — SPEC §3.1 #4, §5.3.
//! For DocClaim facts only: span hash unchanged -> Valid; else if the
//! referenced symbol_path literally appears in the post-commit source
//! of source_path -> Valid; else -> NeedsRevalidation.

use super::{Decision, RowCtx, Rule};

pub struct R6DocClaim;

impl Rule for R6DocClaim {
    fn rule_id(&self) -> &'static str {
        "R6"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §3.1 #4 + §5.3"
    }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        if ctx.fact.kind != "DocClaim" {
            return None;
        }
        let post = ctx.post_blob?;
        let needle = ctx.fact.symbol_path.as_bytes();
        let mentions = post.windows(needle.len()).any(|w| w == needle);
        if mentions {
            return Some((
                Decision::Valid,
                r#"{"rule":"R6","reason":"doc_symbol_still_mentioned"}"#.into(),
            ));
        }
        Some((
            Decision::NeedsRevalidation,
            r#"{"rule":"R6","reason":"doc_symbol_not_mentioned"}"#.into(),
        ))
    }
}
