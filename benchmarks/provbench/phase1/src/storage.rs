//! SQLite schema for the Phase 1 invalidator. Single-file, WAL mode.
//! Tables: facts, diff_artifacts, eval_rows, predictions, rule_traces.

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS facts (
    fact_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    body TEXT NOT NULL,
    source_path TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    symbol_path TEXT NOT NULL,
    content_hash_at_observation TEXT NOT NULL,
    labeler_git_sha TEXT NOT NULL,
    raw_json_sha256 TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS diff_artifacts (
    commit_sha TEXT PRIMARY KEY,
    parent_sha TEXT,
    excluded_reason TEXT,
    unified_diff TEXT,
    raw_json_sha256 TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS eval_rows (
    row_index INTEGER PRIMARY KEY,
    fact_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    ground_truth TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS predictions (
    row_index INTEGER PRIMARY KEY REFERENCES eval_rows(row_index),
    fact_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    ground_truth TEXT NOT NULL,
    prediction TEXT NOT NULL CHECK (prediction IN ('valid','stale','needs_revalidation')),
    request_id TEXT NOT NULL,
    wall_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS rule_traces (
    row_index INTEGER PRIMARY KEY REFERENCES eval_rows(row_index),
    rule_id TEXT NOT NULL,
    spec_ref TEXT NOT NULL,
    reason_code TEXT NOT NULL,
    evidence_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_eval_rows_commit ON eval_rows(commit_sha);
CREATE INDEX IF NOT EXISTS idx_predictions_commit ON predictions(commit_sha);
"#;

/// Open the phase1 SQLite database, enabling WAL mode and foreign keys.
///
/// The schema is idempotent (`CREATE TABLE IF NOT EXISTS`) so re-opening
/// an existing database leaves data intact. Tables: `facts`,
/// `diff_artifacts`, `eval_rows`, `predictions`, `rule_traces`. Indexes:
/// `idx_eval_rows_commit`, `idx_predictions_commit`.
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}
