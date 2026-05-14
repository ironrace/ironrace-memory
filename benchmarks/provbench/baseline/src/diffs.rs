//! Per-commit diff artifact loader. Reads the labeler's
//! `*.diffs/<sha>.json` artifacts into a `commit_sha → DiffArtifact` map.
//!
//! Schema is copied independently from the labeler crate (see SPEC §6.1).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Per-commit diff artifact. Two shapes:
/// - `Included`: a normal commit with a parent and a unified diff
///   materialized at `-U999999` full file context.
/// - `Excluded`: a commit that has no parent (`excluded == "no_parent"`)
///   or is T₀ itself (`excluded == "t0"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum DiffArtifact {
    Included {
        commit_sha: String,
        parent_sha: String,
        unified_diff: String,
    },
    Excluded {
        commit_sha: String,
        /// `"t0"` or `"no_parent"`.
        excluded: String,
    },
}

/// Load all `<sha>.json` artifacts under `dir` into a
/// `commit_sha → DiffArtifact` map.
///
/// Non-`*.json` entries are skipped. Errors include the offending file
/// path for context.
pub fn load_diffs_dir(dir: &Path) -> Result<HashMap<String, DiffArtifact>> {
    let mut map = HashMap::new();
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("open diffs dir {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read diff {}", path.display()))?;
        let artifact: DiffArtifact = serde_json::from_str(&text)
            .with_context(|| format!("parse diff {}", path.display()))?;
        let sha = match &artifact {
            DiffArtifact::Included { commit_sha, .. } => commit_sha.clone(),
            DiffArtifact::Excluded { commit_sha, .. } => commit_sha.clone(),
        };
        map.insert(sha, artifact);
    }
    Ok(map)
}
