//! Preflight worst-case cost estimator (stub).
//!
//! Task 5 ships a conservative placeholder that uses only `rows.len()`
//! and the SPEC §6.2 / §15 price snapshot. Task 6 replaces this with a
//! schema-derived estimator that reads each row's diff and fact body to
//! produce a tighter bound.

use crate::diffs::DiffArtifact;
use crate::facts::FactBody;
use crate::sample::SampledRow;
use std::collections::HashMap;

/// Conservative worst-case cost in USD for invoking the LLM
/// invalidator over `rows`.
///
/// Placeholder semantics: assumes every batch is fully unique
/// (no cache hits), runs at the per-batch token ceiling (~13K input,
/// ~1800 output), and rounds batch count up. Always overestimates —
/// safe for preflight refusal.
pub fn preflight_worst_case_cost(
    rows: &[SampledRow],
    _diffs: &HashMap<String, DiffArtifact>,
    _facts: &HashMap<String, FactBody>,
) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let batches = (rows.len() as f64 / crate::constants::MAX_FACTS_PER_BATCH as f64).ceil();
    // Worst-case input ~13K tokens uncached; output ~1800 tokens (schema bound).
    let cost_per_batch = 13_000.0 / 1_000_000.0
        * crate::constants::PRICE_INPUT_UNCACHED_USD_PER_MTOK
        + 1_800.0 / 1_000_000.0 * crate::constants::PRICE_OUTPUT_USD_PER_MTOK;
    batches * cost_per_batch
}
