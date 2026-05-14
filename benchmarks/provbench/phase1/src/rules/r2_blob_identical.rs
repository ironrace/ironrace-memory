//! R2 blob_identical — SPEC §5.3.
//! Post-commit source blob hash == T0 source blob hash -> Valid fast path.

use super::{Decision, RowCtx, Rule};
use sha2::{Digest, Sha256};

pub struct R2BlobIdentical;

impl Rule for R2BlobIdentical {
    fn rule_id(&self) -> &'static str {
        "R2"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §5.3"
    }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        let (Some(post), Some(t0)) = (ctx.post_blob, ctx.t0_blob) else {
            return None;
        };
        if post == t0 {
            return Some((
                Decision::Valid,
                r#"{"rule":"R2","reason":"blob_identical"}"#.into(),
            ));
        }
        let post_hash = sha256_hex(post);
        let t0_hash = sha256_hex(t0);
        if post_hash == t0_hash {
            return Some((
                Decision::Valid,
                format!(
                    r#"{{"rule":"R2","reason":"blob_hash_identical","sha256":"{}"}}"#,
                    post_hash
                ),
            ));
        }
        None
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
