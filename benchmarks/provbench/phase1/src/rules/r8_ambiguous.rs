//! R8 ambiguous — SPEC §4, §5.2.
//! Symbol present elsewhere with low/tied similarity -> NeedsRevalidation.
//! For v1, R7 already handles the >=0.6 case; R8 fires when the symbol
//! literal appears in a different file but R7 didn't match a candidate
//! confidently.

use super::{Decision, RowCtx, Rule};

pub struct R8Ambiguous;

impl Rule for R8Ambiguous {
    fn rule_id(&self) -> &'static str {
        "R8"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §4 + §5.2"
    }
    fn classify(&self, _ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        // v1: conservative — defer to R9. Empirical tuning may move rows here.
        None
    }
}
