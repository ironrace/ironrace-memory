//! Real ONNX cross-encoder. Mirrors `ironrace-embed/src/embedder.rs`'s shape.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use ndarray::{Array2, ArrayD, IxDyn};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::TensorRef;
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;

use crate::output::extract_logits;
use crate::scorer::RerankerScorer;

/// HuggingFace repo id.
const HF_MODEL_REPO: &str = "BAAI/bge-reranker-base";

/// Local cache subdirectory under `~/.ironrace/models/`.
const MODEL_DIR_NAME: &str = "bge-reranker-base";

/// Max input pair length in tokens.
const MAX_SEQ_LEN: usize = 512;

/// How many pairs to score per ONNX call.
const BATCH_SIZE: usize = 16;

/// SHA-256 of the ONNX file. Empty string = unpinned (only valid with the
/// `unpinned-checksums` Cargo feature).
const MODEL_ONNX_SHA256: &str = "";

/// SHA-256 of tokenizer.json.
const TOKENIZER_JSON_SHA256: &str = "";

/// Cache dir: `~/.ironrace/models/bge-reranker-base/`.
pub fn model_cache_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory. Set HOME env var."))?;
    Ok(home.join(".ironrace").join("models").join(MODEL_DIR_NAME))
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).context("Failed to read file for checksum")?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_or_unpinned(path: &Path, expected: &str, label: &str) -> Result<()> {
    if expected.is_empty() {
        #[cfg(feature = "unpinned-checksums")]
        {
            tracing::warn!(
                "WARN: unpinned checksums for {label} (path={})",
                path.display()
            );
            return Ok(());
        }
        #[cfg(not(feature = "unpinned-checksums"))]
        {
            anyhow::bail!(
                "{label} checksum is unpinned. Build with --features unpinned-checksums for dev only.",
            );
        }
    }
    let got = sha256_file(path)?;
    if got != expected {
        anyhow::bail!("{label} checksum mismatch.\n  Expected: {expected}\n  Got:      {got}",);
    }
    Ok(())
}

/// Probe the local cache for the ONNX model file. Returns the path if found.
fn find_local_onnx(dir: &Path) -> Option<PathBuf> {
    for candidate in ["model.onnx", "onnx/model.onnx", "onnx/model_quantized.onnx"] {
        let p = dir.join(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Download the model + tokenizer if not cached. Returns (onnx_path, tokenizer_path).
fn ensure_downloaded() -> Result<(PathBuf, PathBuf)> {
    let dir = model_cache_dir()?;
    std::fs::create_dir_all(&dir).context("create model cache dir")?;

    let tokenizer_path = dir.join("tokenizer.json");
    if !tokenizer_path.exists() {
        eprintln!("downloading reranker tokenizer (one-time)…");
        let api = hf_hub::api::sync::Api::new()?;
        let repo = api.model(HF_MODEL_REPO.to_string());
        let downloaded = repo
            .get("tokenizer.json")
            .context("hf-hub: tokenizer.json")?;
        std::fs::copy(&downloaded, &tokenizer_path)?;
    }
    verify_or_unpinned(&tokenizer_path, TOKENIZER_JSON_SHA256, "tokenizer.json")?;

    if let Some(onnx) = find_local_onnx(&dir) {
        verify_or_unpinned(&onnx, MODEL_ONNX_SHA256, "model.onnx")?;
        return Ok((onnx, tokenizer_path));
    }

    eprintln!("downloading reranker ONNX model (~280MB, one-time)…");
    let api = hf_hub::api::sync::Api::new()?;
    let repo = api.model(HF_MODEL_REPO.to_string());
    let onnx_dest = dir.join("model.onnx");
    let mut downloaded: Option<PathBuf> = None;
    for candidate in ["model.onnx", "onnx/model.onnx"] {
        if let Ok(p) = repo.get(candidate) {
            downloaded = Some(p);
            break;
        }
    }
    let downloaded = downloaded.ok_or_else(|| {
        anyhow::anyhow!(
            "Neither 'model.onnx' nor 'onnx/model.onnx' found in {HF_MODEL_REPO}. \
             Manually export with `optimum-cli export onnx --model {HF_MODEL_REPO}` \
             and place the result at {}.",
            onnx_dest.display()
        )
    })?;
    std::fs::copy(&downloaded, &onnx_dest)?;
    verify_or_unpinned(&onnx_dest, MODEL_ONNX_SHA256, "model.onnx")?;
    Ok((onnx_dest, tokenizer_path))
}

/// Real cross-encoder backed by ONNX Runtime.
///
/// `Session::run` requires `&mut self`, but the `RerankerScorer` trait scores
/// through `&self`. We wrap the session in a `Mutex` so concurrent callers
/// serialize their inference calls (matches the underlying ORT contract).
pub struct Reranker {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl Reranker {
    pub fn new() -> Result<Self> {
        let (onnx_path, tokenizer_path) = ensure_downloaded()?;
        let session = Session::builder()
            .map_err(|e| anyhow::anyhow!("Failed to create ONNX session builder: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow::anyhow!("Failed to set optimization level: {e}"))?
            .commit_from_file(&onnx_path)
            .with_context(|| format!("Failed to load ONNX model: {}", onnx_path.display()))?;
        let tokenizer =
            Tokenizer::from_file(&tokenizer_path).map_err(|e| anyhow::anyhow!("tokenizer: {e}"))?;
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    fn encode_batch(&self, query: &str, passages: &[&str]) -> Result<(Array2<i64>, Array2<i64>)> {
        let pairs: Vec<(String, String)> = passages
            .iter()
            .map(|p| (query.to_string(), (*p).to_string()))
            .collect();
        let encs = self
            .tokenizer
            .encode_batch(pairs, true)
            .map_err(|e| anyhow::anyhow!("tokenizer encode: {e}"))?;
        let n = encs.len();
        let max_len = encs
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(0)
            .min(MAX_SEQ_LEN);

        let mut ids = Array2::<i64>::zeros((n, max_len));
        let mut mask = Array2::<i64>::zeros((n, max_len));
        for (i, e) in encs.iter().enumerate() {
            let src_ids = e.get_ids();
            let src_mask = e.get_attention_mask();
            let take = src_ids.len().min(max_len);
            for j in 0..take {
                ids[(i, j)] = src_ids[j] as i64;
                mask[(i, j)] = src_mask[j] as i64;
            }
        }
        Ok((ids, mask))
    }
}

impl RerankerScorer for Reranker {
    fn score_pairs(&self, query: &str, passages: &[&str]) -> Result<Vec<f32>> {
        if passages.is_empty() {
            return Ok(Vec::new());
        }
        let mut all = Vec::with_capacity(passages.len());
        for chunk in passages.chunks(BATCH_SIZE) {
            let (ids, mask) = self.encode_batch(query, chunk)?;
            let ids_tensor = TensorRef::from_array_view(ids.view())
                .map_err(|e| anyhow::anyhow!("Failed to create input_ids tensor: {e}"))?;
            let mask_tensor = TensorRef::from_array_view(mask.view())
                .map_err(|e| anyhow::anyhow!("Failed to create attention_mask tensor: {e}"))?;
            let mut session = self
                .session
                .lock()
                .map_err(|e| anyhow::anyhow!("session mutex poisoned: {e}"))?;
            let outputs = session
                .run(ort::inputs![
                    "input_ids" => ids_tensor,
                    "attention_mask" => mask_tensor,
                ])
                .map_err(|e| anyhow::anyhow!("ONNX inference failed: {e}"))?;

            // Most rerankers expose "logits"; fall back to first output by index.
            let (shape, data): (&ort::value::Shape, &[f32]) = if let Some(val) =
                outputs.get("logits")
            {
                val.try_extract_tensor()
                    .map_err(|e| anyhow::anyhow!("Failed to extract logits: {e}"))?
            } else {
                let val = &outputs[0];
                val.try_extract_tensor()
                    .map_err(|e| anyhow::anyhow!("Failed to extract first output tensor: {e}"))?
            };

            let dims: Vec<usize> = shape.iter().map(|&d| d as usize).collect();
            let arr = ArrayD::from_shape_vec(IxDyn(&dims), data.to_vec())
                .map_err(|e| anyhow::anyhow!("Failed to reshape ONNX output: {e}"))?;
            let scores = extract_logits(&arr)?;
            all.extend(scores);
        }
        Ok(all)
    }
}
