//! Commit-grouped runner. Reads `eval_rows` grouped by (commit_sha, source_path),
//! opens the commit tree once via gix, parses touched files once with tree-sitter,
//! runs the rule chain per fact, writes results to SQLite and to JSONL artifacts.

use anyhow::{Context, Result};
use provbench_scoring::PredictionRow;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use crate::facts::FactBody;
use crate::repo::Repo;
use crate::rules::{Decision, RowCtx, RuleChain};

pub struct RunnerOpts<'a> {
    pub db: &'a Connection,
    pub repo: &'a Repo,
    pub t0: &'a str,
    pub rule_set_version: &'a str,
    pub out_predictions: &'a Path,
    pub out_traces: &'a Path,
}

pub fn run(opts: RunnerOpts<'_>) -> Result<RunStats> {
    let chain = RuleChain::default();

    // Load all facts once.
    let mut facts: HashMap<String, FactBody> = HashMap::new();
    {
        let mut stmt = opts.db.prepare(
            "SELECT fact_id, kind, body, source_path, line_start, line_end, \
             symbol_path, content_hash_at_observation, labeler_git_sha FROM facts",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(FactBody {
                fact_id: r.get(0)?,
                kind: r.get(1)?,
                body: r.get(2)?,
                source_path: r.get(3)?,
                line_span: [r.get::<_, i64>(4)? as u64, r.get::<_, i64>(5)? as u64],
                symbol_path: r.get(6)?,
                content_hash_at_observation: r.get(7)?,
                labeler_git_sha: r.get(8)?,
            })
        })?;
        for row in rows {
            let f = row?;
            facts.insert(f.fact_id.clone(), f);
        }
    }

    // T0 blob cache, keyed by source_path.
    let mut t0_blobs: HashMap<String, Option<Vec<u8>>> = HashMap::new();
    // Per-commit tree-listing cache (populated for R7 rename search).
    let mut commit_files_cache: HashMap<String, Vec<String>> = HashMap::new();

    // Stream eval_rows ordered for stable output.
    let mut stmt = opts.db.prepare(
        "SELECT row_index, fact_id, commit_sha, batch_id, ground_truth \
         FROM eval_rows ORDER BY row_index ASC",
    )?;
    let mut rows = stmt.query([])?;
    let mut predictions_f = File::create(opts.out_predictions)?;
    let mut traces_f = File::create(opts.out_traces)?;
    let mut ins_pred = opts.db.prepare(
        "INSERT INTO predictions \
         (row_index, fact_id, commit_sha, batch_id, ground_truth, prediction, request_id, wall_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let mut ins_trace = opts.db.prepare(
        "INSERT INTO rule_traces (row_index, rule_id, spec_ref, reason_code, evidence_json) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    let mut stats = RunStats::default();

    while let Some(r) = rows.next()? {
        let row_index: i64 = r.get(0)?;
        let fact_id: String = r.get(1)?;
        let commit_sha: String = r.get(2)?;
        let batch_id: String = r.get(3)?;
        let ground_truth: String = r.get(4)?;

        let fact = facts
            .get(&fact_id)
            .with_context(|| format!("fact_id {} not in facts table", fact_id))?;

        let t0_blob = t0_blobs
            .entry(fact.source_path.clone())
            .or_insert_with(|| {
                opts.repo
                    .blob_at(opts.t0, &fact.source_path)
                    .unwrap_or(None)
            })
            .clone();
        let post_blob = opts.repo.blob_at(&commit_sha, &fact.source_path)?;

        let commit_files = commit_files_cache
            .entry(commit_sha.clone())
            .or_insert_with(|| opts.repo.list_tree(&commit_sha).unwrap_or_default())
            .clone();

        let started = Instant::now();
        let ctx = RowCtx {
            fact,
            commit_sha: &commit_sha,
            diff: None,
            post_blob: post_blob.as_deref(),
            t0_blob: t0_blob.as_deref(),
            post_tree: None,
            commit_files: &commit_files,
        };
        let (decision, rule_id, spec_ref, evidence) = chain.classify_first_match(&ctx);
        let wall_ms = started.elapsed().as_millis() as u64;

        let pred = decision.as_str().to_string();
        let request_id = format!(
            "phase1:{}:{}:{}",
            opts.rule_set_version, commit_sha, row_index
        );

        ins_pred.execute(params![
            row_index,
            &fact_id,
            &commit_sha,
            &batch_id,
            &ground_truth,
            &pred,
            &request_id,
            wall_ms as i64,
        ])?;
        ins_trace.execute(params![row_index, rule_id, spec_ref, "n/a", &evidence])?;

        let pr_row = PredictionRow {
            fact_id: fact_id.clone(),
            commit_sha: commit_sha.clone(),
            batch_id: batch_id.clone(),
            ground_truth: ground_truth.clone(),
            prediction: pred.clone(),
            request_id: request_id.clone(),
            wall_ms,
        };
        writeln!(predictions_f, "{}", serde_json::to_string(&pr_row)?)?;
        let trace_obj = serde_json::json!({
            "row_index": row_index,
            "rule_id": rule_id,
            "spec_ref": spec_ref,
            "evidence": serde_json::from_str::<serde_json::Value>(&evidence).unwrap_or(serde_json::Value::Null),
        });
        writeln!(traces_f, "{}", trace_obj)?;

        stats.processed += 1;
        match decision {
            Decision::Valid => stats.valid += 1,
            Decision::Stale => stats.stale += 1,
            Decision::NeedsRevalidation => stats.needs_reval += 1,
        }
    }
    Ok(stats)
}

#[derive(Default, Debug)]
pub struct RunStats {
    pub processed: u64,
    pub valid: u64,
    pub stale: u64,
    pub needs_reval: u64,
}
