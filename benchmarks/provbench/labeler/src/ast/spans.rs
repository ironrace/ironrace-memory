//! Byte-span + line-span types and content hashing used by every fact
//! kind. SHA-256 is used everywhere — never `Hash`/`u64` — so labels are
//! reproducible across runs and machines.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub byte_range: std::ops::Range<usize>,
    pub line_start: u32, // 1-based inclusive
    pub line_end: u32,   // 1-based inclusive
}

pub fn content_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// Hash only the slice within `span` from `source`. Convenience for the
/// labeling rule engine.
pub fn span_hash(source: &[u8], span: &Span) -> String {
    content_hash(&source[span.byte_range.clone()])
}
