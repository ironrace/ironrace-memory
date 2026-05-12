//! Symbol resolution traits. Replay now uses a commit-local tree-sitter index;
//! the rust-analyzer backend remains for ignored tooling tests and future
//! semantic-resolution work. Python (held-out) will get a tree-sitter +
//! import-graph implementation later — keep the trait language-agnostic.

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
