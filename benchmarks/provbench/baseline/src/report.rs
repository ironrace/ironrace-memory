//! Re-export shim — moved to `provbench-scoring`.
pub use provbench_scoring::report::*;

/// Backwards-compat wrapper preserving the historical `provbench-baseline score`
/// entry point.
pub fn score_run(run_dir: &std::path::Path) -> anyhow::Result<()> {
    provbench_scoring::report::score_llm_baseline_run(run_dir)
}
