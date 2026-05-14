//! Deterministic JSONL output with labeler-SHA stamping.
//!
//! Determinism contract: byte-identical output across runs given the
//! same labeler git SHA, repo, and T₀.

use crate::label::Label;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::Path;

/// One on-disk row, prior to labeler-SHA stamping.
///
/// The serialized form on disk also carries `labeler_git_sha` (added by
/// [`write_jsonl`]). Reading code that only needs the data triple can
/// deserialize into this type and ignore the extra field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputRow {
    pub fact_id: String,
    pub commit_sha: String,
    pub label: Label,
}

#[derive(Debug, Serialize)]
struct Stamped<'a> {
    fact_id: &'a str,
    commit_sha: &'a str,
    label: &'a Label,
    labeler_git_sha: &'a str,
}

/// Write `rows` as JSONL, sorted by `(fact_id, commit_sha)` and stamped
/// with `labeler_git_sha` on every line.
///
/// Output is byte-deterministic given the same inputs: stable sort key,
/// fixed serialization order, no timestamps, single trailing newline per
/// row.
pub fn write_jsonl(path: &Path, rows: &[OutputRow], labeler_git_sha: &str) -> Result<()> {
    let mut sorted: Vec<&OutputRow> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        a.fact_id
            .cmp(&b.fact_id)
            .then_with(|| a.commit_sha.cmp(&b.commit_sha))
    });
    let mut f = std::fs::File::create(path)?;
    for row in sorted {
        let stamped = Stamped {
            fact_id: &row.fact_id,
            commit_sha: &row.commit_sha,
            label: &row.label,
            labeler_git_sha,
        };
        serde_json::to_writer(&mut f, &stamped)?;
        f.write_all(b"\n")?;
    }
    f.flush()?;
    Ok(())
}

/// Read a JSONL corpus produced by [`write_jsonl`] back into
/// [`OutputRow`] values. Lines that are empty after trimming are skipped
/// (a trailing `"\n"` in the file is normal). The stamped
/// `labeler_git_sha` field on disk is ignored by serde because
/// [`OutputRow`] does not declare it; callers that need the stamp must
/// parse the line into a [`serde_json::Value`] separately.
pub fn read_jsonl(path: &Path) -> Result<Vec<OutputRow>> {
    let f = std::fs::File::open(path)?;
    let mut rows = Vec::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(&line)?);
    }
    Ok(rows)
}

/// One T₀ fact body row, emitted by the `emit-facts` subcommand. Mirrors
/// the baseline-side `FactBody` schema (single source of truth — Task 5
/// in the Phase 0c plan deserializes JSONL produced by
/// [`write_facts_jsonl`] back into this struct).
///
/// `body` is the SPEC §3 single-line claim string (e.g. `"function
/// crate::add has parameters (a: i32, b: i32) with return type i32"`).
/// `line_span` is `[start, end]` with 1-based inclusive line numbers.
/// `content_hash_at_observation` is the 64-char lowercase hex SHA-256
/// of the fact's bound span at T₀.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactBodyRow {
    pub fact_id: String,
    pub kind: String,
    pub body: String,
    pub source_path: String,
    pub line_span: [u32; 2],
    pub symbol_path: String,
    pub content_hash_at_observation: String,
}

#[derive(Debug, Serialize)]
struct StampedFactBody<'a> {
    fact_id: &'a str,
    kind: &'a str,
    body: &'a str,
    source_path: &'a str,
    line_span: [u32; 2],
    symbol_path: &'a str,
    content_hash_at_observation: &'a str,
    labeler_git_sha: &'a str,
}

/// Write `rows` as JSONL, sorted by `fact_id` and stamped with
/// `labeler_git_sha`. Byte-deterministic given the same inputs.
pub fn write_facts_jsonl(path: &Path, rows: &[FactBodyRow], labeler_git_sha: &str) -> Result<()> {
    let mut sorted: Vec<&FactBodyRow> = rows.iter().collect();
    sorted.sort_by(|a, b| a.fact_id.cmp(&b.fact_id));
    let mut f = std::fs::File::create(path)?;
    for row in sorted {
        let stamped = StampedFactBody {
            fact_id: &row.fact_id,
            kind: &row.kind,
            body: &row.body,
            source_path: &row.source_path,
            line_span: row.line_span,
            symbol_path: &row.symbol_path,
            content_hash_at_observation: &row.content_hash_at_observation,
            labeler_git_sha,
        };
        serde_json::to_writer(&mut f, &stamped)?;
        f.write_all(b"\n")?;
    }
    f.flush()?;
    Ok(())
}
