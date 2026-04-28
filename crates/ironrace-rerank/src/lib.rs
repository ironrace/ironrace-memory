//! ironrace-rerank: ONNX cross-encoder reranker.
//!
//! Exposes the `RerankerScorer` trait (implementations: real ONNX `Reranker`
//! and test-only `NoopScorer`) plus the `Reranker` struct itself for callers
//! wiring it into a search pipeline.

pub mod output;
pub mod reranker;
pub mod scorer;

pub use reranker::{model_cache_dir, Reranker};
pub use scorer::{NoopScorer, RerankerScorer};
