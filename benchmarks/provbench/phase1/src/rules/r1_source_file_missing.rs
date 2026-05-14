//! R1 source_file_missing — SPEC §5.1.
//! fact.source_path absent in commit tree -> Stale.

use super::{Decision, RowCtx, Rule};

pub struct R1SourceFileMissing;

impl Rule for R1SourceFileMissing {
    fn rule_id(&self) -> &'static str {
        "R1"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §5.1"
    }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        if ctx.post_blob.is_none() {
            return Some((
                Decision::Stale,
                serde_json::json!({
                    "rule": "R1",
                    "reason": "stale_source_deleted",
                    "source_path": ctx.fact.source_path,
                })
                .to_string(),
            ));
        }
        None
    }
}
