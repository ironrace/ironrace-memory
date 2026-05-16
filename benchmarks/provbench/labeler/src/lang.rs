//! Language enum + per-path dispatch. Stable extension order so any
//! iteration over `Language::source_extensions()` is deterministic.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
}

impl Language {
    /// Detect language from a path's extension. Returns `None` for paths
    /// that are not source files (e.g., `.md`, `.toml`).
    pub fn for_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Some(Language::Rust),
            Some("py") => Some(Language::Python),
            _ => None,
        }
    }

    /// Stable lexicographic order. Replay iterates this list to
    /// build a deterministic per-language file partition.
    pub fn source_extensions() -> &'static [&'static str] {
        &["py", "rs"]
    }

    pub fn extension(self) -> &'static str {
        match self {
            Language::Rust => "rs",
            Language::Python => "py",
        }
    }
}
