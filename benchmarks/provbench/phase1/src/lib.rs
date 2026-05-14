//! Phase 1 rules-based structural invalidator for ProvBench.
//!
//! Consumes the labeler's `*.facts.jsonl` + per-commit diff artifacts, evaluates
//! the row set pinned by `--baseline-run/predictions.jsonl`, and emits
//! `predictions.jsonl` (matches `provbench_scoring::PredictionRow`
//! byte-for-byte) + `rule_traces.jsonl`.

pub mod baseline_run;
pub mod diffs;
pub mod facts;
pub mod parse;
pub mod repo;
pub mod rules;
pub mod runner;
pub mod similarity;
pub mod storage;
