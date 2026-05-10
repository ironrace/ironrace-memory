//! Symbol resolution traits. Phase 0b uses rust-analyzer for Rust;
//! Python (held-out) will get a tree-sitter + import-graph implementation
//! later — keep the trait language-agnostic.

pub mod rust_analyzer;

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLocation {
    pub file: PathBuf,
    pub line: u32,
}

pub trait SymbolResolver {
    fn resolve(&mut self, qualified_name: &str) -> anyhow::Result<Option<ResolvedLocation>>;
}
