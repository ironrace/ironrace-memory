//! R0 diff_excluded — SPEC §5.
//! Orphan / missing-parent / excluded diff artifact and the file cannot
//! be located at the commit -> NeedsRevalidation.

use super::{Decision, RowCtx, Rule};

pub struct R0DiffExcluded;

impl Rule for R0DiffExcluded {
    fn rule_id(&self) -> &'static str {
        "R0"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §5"
    }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        let excluded = ctx
            .diff
            .and_then(|d| d.excluded_reason.as_deref())
            .is_some();
        if excluded && ctx.post_blob.is_none() && !ctx.commit_files.is_empty() {
            return Some((
                Decision::NeedsRevalidation,
                serde_json::json!({ "rule": "R0", "reason": "diff_excluded_or_orphan" })
                    .to_string(),
            ));
        }
        None
    }
}
