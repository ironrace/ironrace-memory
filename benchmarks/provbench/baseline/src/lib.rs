//! Phase 0c LLM-as-invalidator baseline for ProvBench-CodeContext.
//!
//! Frozen contract: `benchmarks/provbench/SPEC.md`. Edits to anything frozen
//! by the SPEC (prompt text, model pin, batching, scoring) must be matched by
//! a SPEC §11 entry and a new freeze hash.
//!
//! This is a skeleton crate. CLI subcommand implementations land in later
//! tasks (`sample` — Task 5, `run` — Task 8, `score` — Task 9).

pub mod budget;
pub mod client;
pub mod constants;
pub mod diffs;
pub mod facts;
pub mod manifest;
pub mod metrics;
pub mod prompt;
pub mod report;
pub mod runner;
pub mod sample;
