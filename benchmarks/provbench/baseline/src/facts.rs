//! T₀ fact-body loader. Reads the labeler's `*.facts.jsonl` artifact
//! into a `fact_id → FactBody` map.
//!
//! Schema is copied independently from the labeler crate (see SPEC §3 /
//! §6.1). The `labeler_git_sha` field stamped on every on-disk row is
//! intentionally ignored here — it is carried separately in
//! [`crate::manifest::SampleManifest::labeler_git_sha`].

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

pub use crate::prompt::FactBody;

#[derive(Deserialize)]
struct FactBodyOnDisk {
    fact_id: String,
    kind: String,
    body: String,
    source_path: String,
    line_span: [u32; 2],
    symbol_path: String,
    content_hash_at_observation: String,
    /// Ignored on load — carried separately by the manifest.
    #[serde(default, rename = "labeler_git_sha")]
    _stamp: Option<String>,
}

/// Load all fact bodies from a JSONL file into a `fact_id → FactBody` map.
///
/// Empty lines (e.g. the trailing newline) are skipped. Errors include
/// the path for context.
pub fn load_facts(path: &Path) -> Result<HashMap<String, FactBody>> {
    let f = std::fs::File::open(path).with_context(|| format!("open facts {}", path.display()))?;
    let mut map = HashMap::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let r: FactBodyOnDisk = serde_json::from_str(&line)
            .with_context(|| format!("parse fact row in {}", path.display()))?;
        map.insert(
            r.fact_id.clone(),
            FactBody {
                fact_id: r.fact_id,
                kind: r.kind,
                body: r.body,
                source_path: r.source_path,
                line_span: r.line_span,
                symbol_path: r.symbol_path,
                content_hash_at_observation: r.content_hash_at_observation,
            },
        );
    }
    Ok(map)
}
