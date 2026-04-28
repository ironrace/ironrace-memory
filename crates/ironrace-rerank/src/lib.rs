//! ironrace-rerank: ONNX cross-encoder reranker.
//!
//! Exposes the `RerankerScorer` trait (implementations: real ONNX `Reranker`
//! and test-only `NoopScorer`) plus the `Reranker` struct itself for callers
//! wiring it into a search pipeline.

pub mod output;
pub mod scorer;

pub use scorer::{NoopScorer, RerankerScorer};

// `reranker` module is added in Task 5.
