//! Tooling-pin verification per SPEC §13.1.
//!
//! Phase 0b labels are invalid unless every external tool used at label
//! time matches the binary content hash recorded in the spec freeze. A
//! version-string match alone is **not** sufficient — distros patch.

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct ExpectedTool {
    pub name: &'static str,
    pub version_hint: &'static str,
    pub sha256_hex: &'static str,
}

/// rust-analyzer 1.85.0 (4d91de4e 2025-02-17), rustup stable-aarch64-apple-darwin.
pub const RUST_ANALYZER: ExpectedTool = ExpectedTool {
    name: "rust-analyzer",
    version_hint: "1.85.0 (4d91de4e 2025-02-17)",
    sha256_hex: "f85740bfa5b9136e9053768c015c31a6c7556f7cfe44f7f9323965034e1f9aee",
};

/// tree-sitter 0.25.6 (Homebrew, /opt/homebrew/bin/tree-sitter).
pub const TREE_SITTER: ExpectedTool = ExpectedTool {
    name: "tree-sitter",
    version_hint: "0.25.6",
    sha256_hex: "3e82f0982232f68fd5b0192caf4bb06064cc034f837552272eec8d67014edc5c",
};

pub fn verify_binary_hash(path: &Path, expected: &ExpectedTool) -> Result<()> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read {} at {}", expected.name, path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected.sha256_hex {
        return Err(anyhow!(
            "tooling hash mismatch for {}: expected {} (version {}), got {}",
            expected.name,
            expected.sha256_hex,
            expected.version_hint,
            actual
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ResolvedTooling {
    pub rust_analyzer: std::path::PathBuf,
    pub tree_sitter: std::path::PathBuf,
}

fn resolve_binary(name: &str, fallback: &str) -> Result<std::path::PathBuf> {
    let path = match which::which(name) {
        Ok(p) => p,
        Err(_) => std::path::PathBuf::from(fallback),
    };
    if !path.exists() {
        anyhow::bail!("{name} not found on PATH and not present at {fallback}");
    }
    Ok(path)
}

pub fn resolve_from_env() -> Result<ResolvedTooling> {
    let rust_analyzer = resolve_binary("rust-analyzer", "/opt/homebrew/bin/rust-analyzer")?;
    let tree_sitter = resolve_binary("tree-sitter", "/opt/homebrew/bin/tree-sitter")?;
    verify_binary_hash(&rust_analyzer, &RUST_ANALYZER)?;
    verify_binary_hash(&tree_sitter, &TREE_SITTER)?;
    Ok(ResolvedTooling {
        rust_analyzer,
        tree_sitter,
    })
}
