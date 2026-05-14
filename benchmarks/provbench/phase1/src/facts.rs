//! Loader for `<repo>.facts.jsonl` artifacts emitted by `provbench-labeler`.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Row shape returned by the duplicate-fact_id SELECT below.
/// Columns: (kind, body, source_path, line_start, line_end, symbol_path,
/// content_hash_at_observation, labeler_git_sha, raw_json_sha256).
type ExistingFactRow = (
    String,
    String,
    String,
    i64,
    i64,
    String,
    String,
    String,
    String,
);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactBody {
    pub fact_id: String,
    pub kind: String,
    pub body: String,
    pub source_path: String,
    pub line_span: [u64; 2],
    pub symbol_path: String,
    pub content_hash_at_observation: String,
    pub labeler_git_sha: String,
}

pub fn ingest(db: &Connection, path: &Path) -> Result<usize> {
    let f = File::open(path).with_context(|| format!("opening facts file {}", path.display()))?;
    let mut stmt = db.prepare(
        "INSERT OR ABORT INTO facts (fact_id, kind, body, source_path, line_start, line_end, \
         symbol_path, content_hash_at_observation, labeler_git_sha, raw_json_sha256) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    let mut count = 0usize;
    for (i, line) in BufReader::new(f).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let hash = sha256_hex(line.as_bytes());
        let fact: FactBody =
            serde_json::from_str(&line).with_context(|| format!("parsing facts line {}", i + 1))?;
        // Duplicate fact_id: allowed only when semantic fields match exactly.
        let existing: Option<ExistingFactRow> = db
            .query_row(
                "SELECT kind, body, source_path, line_start, line_end, symbol_path, \
                 content_hash_at_observation, labeler_git_sha, raw_json_sha256 \
                 FROM facts WHERE fact_id = ?1",
                params![&fact.fact_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ))
                },
            )
            .ok();
        if let Some((k, b, sp, ls, le, sym, ch, lsha, _rj)) = existing {
            anyhow::ensure!(
                k == fact.kind
                    && b == fact.body
                    && sp == fact.source_path
                    && ls == fact.line_span[0] as i64
                    && le == fact.line_span[1] as i64
                    && sym == fact.symbol_path
                    && ch == fact.content_hash_at_observation
                    && lsha == fact.labeler_git_sha,
                "duplicate fact_id {} with mismatched fields at line {}",
                fact.fact_id,
                i + 1
            );
            continue;
        }
        stmt.execute(params![
            &fact.fact_id,
            &fact.kind,
            &fact.body,
            &fact.source_path,
            fact.line_span[0] as i64,
            fact.line_span[1] as i64,
            &fact.symbol_path,
            &fact.content_hash_at_observation,
            &fact.labeler_git_sha,
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
