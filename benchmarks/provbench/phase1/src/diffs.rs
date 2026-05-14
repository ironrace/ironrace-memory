//! Loader for per-commit `<sha>.json` diff artifacts.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitDiff {
    pub commit_sha: String,
    pub parent_sha: Option<String>,
    #[serde(default)]
    pub excluded_reason: Option<String>,
    #[serde(default)]
    pub unified_diff: Option<String>,
}

pub fn ingest(db: &Connection, dir: &Path) -> Result<usize> {
    let mut stmt = db.prepare(
        "INSERT OR REPLACE INTO diff_artifacts \
         (commit_sha, parent_sha, excluded_reason, unified_diff, raw_json_sha256) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut count = 0usize;
    for entry in
        fs::read_dir(dir).with_context(|| format!("reading diffs dir {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let bytes =
            fs::read(&path).with_context(|| format!("reading diff file {}", path.display()))?;
        let hash = sha256_hex(&bytes);
        let cd: CommitDiff = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing diff {}", path.display()))?;
        stmt.execute(params![
            &cd.commit_sha,
            &cd.parent_sha,
            &cd.excluded_reason,
            &cd.unified_diff,
            &hash,
        ])?;
        count += 1;
    }
    Ok(count)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
