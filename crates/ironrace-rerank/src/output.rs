//! Cross-encoder ONNX output extractor.
//!
//! Different reranker exports surface scores under different shapes. We
//! support three layouts for an N-pair batch:
//!   - `[N]`     — one logit per pair, flat.
//!   - `[N, 1]`  — column vector (most common).
//!   - `[1, N]`  — row vector (some exports).
//!
//! Anything else is rejected — callers should not silently coerce a multi-
//! class head's argmax into a relevance score.

use anyhow::{bail, Result};
use ndarray::ArrayD;

pub fn extract_logits(out: &ArrayD<f32>) -> Result<Vec<f32>> {
    match out.ndim() {
        1 => Ok(out.iter().copied().collect()),
        2 => {
            let shape = out.shape();
            if shape[0] == 1 || shape[1] == 1 {
                Ok(out.iter().copied().collect())
            } else {
                bail!(
                    "unsupported reranker output shape {:?}: expected [N], [N,1], or [1,N]",
                    shape
                );
            }
        }
        n => bail!("unsupported reranker output rank {}", n),
    }
}
