//! Deterministic JSONL output with labeler-SHA stamping.
//!
//! Determinism contract: byte-identical output across runs given the
//! same labeler git SHA, repo, and T₀.

use crate::label::Label;
use anyhow::Result;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
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

pub fn current_labeler_sha() -> Result<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;
    if !out.status.success() {
        anyhow::bail!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}
