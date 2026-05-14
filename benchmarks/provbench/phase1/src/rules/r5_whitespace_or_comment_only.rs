//! R5 whitespace_or_comment_only — SPEC §5.3.
//! Span byte content differs but tokenized form (comments+whitespace
//! stripped) is unchanged -> Valid.

use super::{Decision, RowCtx, Rule};
use crate::parse::{rust_tokens_equivalent, FileKind};

pub struct R5WhitespaceOrCommentOnly;

impl Rule for R5WhitespaceOrCommentOnly {
    fn rule_id(&self) -> &'static str {
        "R5"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §5.3"
    }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        let (Some(post), Some(t0)) = (ctx.post_blob, ctx.t0_blob) else {
            return None;
        };
        if post == t0 {
            return None;
        } // R2 handles this; defensive.
        let path = &ctx.fact.source_path;
        let kind = if path.ends_with(".rs") {
            FileKind::Rust
        } else if path.ends_with(".md") || path.ends_with(".markdown") {
            FileKind::Markdown
        } else {
            FileKind::Other
        };
        let equiv = match kind {
            FileKind::Rust => rust_tokens_equivalent(t0, post),
            FileKind::Markdown => {
                let a = String::from_utf8_lossy(t0)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let b = String::from_utf8_lossy(post)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                a == b
            }
            FileKind::Other => false,
        };
        if equiv {
            return Some((
                Decision::Valid,
                format!(
                    r#"{{"rule":"R5","reason":"whitespace_or_comment_only","kind":"{:?}"}}"#,
                    kind
                ),
            ));
        }
        None
    }
}
