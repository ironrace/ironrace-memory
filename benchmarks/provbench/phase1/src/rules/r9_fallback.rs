//! R9 fallback — SPEC §5.3 final clause.

use super::{Decision, RowCtx, Rule};

pub struct R9Fallback;

impl Rule for R9Fallback {
    fn rule_id(&self) -> &'static str {
        "R9"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §5.3"
    }
    fn classify(&self, _ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        Some((
            Decision::NeedsRevalidation,
            r#"{"rule":"R9","reason":"fallback"}"#.into(),
        ))
    }
}
