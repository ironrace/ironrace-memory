//! Deterministic JSONL output with labeler-SHA stamping.
//!
//! Determinism contract: byte-identical output across runs given the
//! same labeler git SHA, repo, and T₀.

use crate::label::Label;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Write;
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
