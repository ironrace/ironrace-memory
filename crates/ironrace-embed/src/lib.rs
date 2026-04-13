//! ironrace-embed: ONNX sentence embeddings (pure Rust).
//!
//! Provides `Embedder` for ONNX-based sentence embeddings using MiniLM-L6-v2.

pub mod embedder;

pub use embedder::{Embedder, EMBED_DIM};
