//! R4 span_hash_changed — SPEC §5.3.
//! Symbol resolves but post-span content hash != content_hash_at_observation,
//! and R5/R6/R7 didn't fire -> Stale (stale_source_changed).

use super::{Decision, RowCtx, Rule};
use sha2::{Digest, Sha256};

pub struct R4SpanHashChanged;

impl Rule for R4SpanHashChanged {
    fn rule_id(&self) -> &'static str {
        "R4"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §5.3"
    }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        let (Some(post), _) = (ctx.post_blob, ctx.t0_blob) else {
            return None;
        };
        let span = extract_span(post, ctx.fact.line_span);
        let hash = sha256_hex(&span);
        if hash != ctx.fact.content_hash_at_observation {
            return Some((
                Decision::Stale,
                format!(
                    r#"{{"rule":"R4","reason":"stale_source_changed","post_hash":"{}"}}"#,
                    hash
                ),
            ));
        }
        None
    }
}

fn extract_span(src: &[u8], span: [u64; 2]) -> Vec<u8> {
    let mut out = Vec::new();
    let (start, end) = (span[0] as usize, span[1] as usize);
    for (i, line) in src.split(|&b| b == b'\n').enumerate() {
        let lineno = i + 1;
        if lineno >= start && lineno <= end {
            out.extend_from_slice(line);
            out.push(b'\n');
        }
        if lineno > end {
            break;
        }
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
