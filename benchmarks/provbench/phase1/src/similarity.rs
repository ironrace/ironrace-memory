//! Rename-candidate similarity (SPEC §5 step 2, frozen threshold 0.6).
//! Jaccard token similarity over whitespace-split tokens.

/// Token-based Jaccard similarity. Symmetric, deterministic, in [0,1].
/// SPEC §5 step 2 frozen threshold: 0.6.
pub fn similarity(a: &str, b: &str) -> f32 {
    use std::collections::HashSet;
    let ta: HashSet<&str> = a.split_whitespace().collect();
    let tb: HashSet<&str> = b.split_whitespace().collect();
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    let inter = ta.intersection(&tb).count() as f32;
    let union = ta.union(&tb).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

pub const RENAME_THRESHOLD: f32 = 0.6;
