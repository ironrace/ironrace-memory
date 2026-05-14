//! Loader for the authoritative eval-row subset.
//! Pins phase1's evaluation to exactly the rows the LLM baseline scored.

use anyhow::{Context, Result};
use provbench_scoring::PredictionRow;
use rusqlite::{params, Connection};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn ingest(db: &Connection, predictions_jsonl: &Path) -> Result<usize> {
    let f = File::open(predictions_jsonl)
        .with_context(|| format!("opening {}", predictions_jsonl.display()))?;
    let mut stmt = db.prepare(
        "INSERT INTO eval_rows (row_index, fact_id, commit_sha, batch_id, ground_truth) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut count = 0usize;
    for (i, line) in BufReader::new(f).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let row: PredictionRow = serde_json::from_str(&line)
            .with_context(|| format!("parsing baseline-run line {}", i + 1))?;
        stmt.execute(params![
            i as i64,
            &row.fact_id,
            &row.commit_sha,
            &row.batch_id,
            &row.ground_truth,
        ])?;
        count += 1;
    }
    Ok(count)
}
