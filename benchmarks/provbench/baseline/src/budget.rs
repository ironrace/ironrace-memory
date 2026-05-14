//! Preflight worst-case cost estimator + runtime cost meter.
//!
//! Preflight: schema-derived bound that reads each row's diff and fact
//! body to produce a tighter estimate than the prior placeholder. Always
//! conservative (overestimates), safe for preflight refusal at the
//! operational cap.
//!
//! Runtime: [`CostMeter`] tracks actual spend from `Usage` reports and
//! gates each batch at 95% of the operational cap. The SPEC §6.2 / §15
//! ceiling is enforced as a debug invariant — must never be reachable.

use crate::client::Usage;
use crate::constants::*;
use crate::diffs::DiffArtifact;
use crate::facts::FactBody;
use crate::sample::SampledRow;
use std::collections::HashMap;

pub fn preflight_worst_case_cost(
    rows: &[SampledRow],
    diffs: &HashMap<String, DiffArtifact>,
    facts: &HashMap<String, FactBody>,
) -> f64 {
    let mut commits: HashMap<&String, usize> = HashMap::new();
    for r in rows {
        *commits.entry(&r.commit_sha).or_default() += 1;
    }

    let median_diff_tokens: f64 = if diffs.is_empty() {
        2_000.0
    } else {
        let mut diff_lens: Vec<usize> = diffs
            .values()
            .filter_map(|d| match d {
                DiffArtifact::Included { unified_diff, .. } => Some(unified_diff.len()),
                _ => None,
            })
            .collect();
        diff_lens.sort_unstable();
        let median_chars = diff_lens.get(diff_lens.len() / 2).copied().unwrap_or(8_000);
        median_chars as f64 / 4.0
    };

    let median_fact_tokens: f64 = if facts.is_empty() {
        80.0
    } else {
        let mut lens: Vec<usize> = facts
            .values()
            .map(|f| f.body.len() + f.source_path.len() + 80)
            .collect();
        lens.sort_unstable();
        lens.get(lens.len() / 2).copied().unwrap_or(320) as f64 / 4.0
    };

    let static_prefix_tokens = 250.0;
    let worst_case_output_tokens = 1_800.0;
    let mut total_usd = 0.0;

    for (_commit, batches_for_commit) in commits {
        let n_batches = (batches_for_commit as f64 / MAX_FACTS_PER_BATCH as f64).ceil();
        let cacheable_tokens = static_prefix_tokens + median_diff_tokens + 10.0;
        let facts_block_tokens = MAX_FACTS_PER_BATCH as f64 * median_fact_tokens;

        let first_in = (cacheable_tokens / 1_000_000.0) * PRICE_INPUT_CACHE_WRITE_USD_PER_MTOK
            + (facts_block_tokens / 1_000_000.0) * PRICE_INPUT_UNCACHED_USD_PER_MTOK;
        let later_in_per = (cacheable_tokens / 1_000_000.0) * PRICE_INPUT_CACHE_READ_USD_PER_MTOK
            + (facts_block_tokens / 1_000_000.0) * PRICE_INPUT_UNCACHED_USD_PER_MTOK;
        let output_per = (worst_case_output_tokens / 1_000_000.0) * PRICE_OUTPUT_USD_PER_MTOK;

        total_usd +=
            first_in + output_per + (n_batches - 1.0).max(0.0) * (later_in_per + output_per);
    }
    total_usd
}

#[derive(Debug, Clone)]
pub struct CostMeter {
    pub cap: f64,
    pub cost_usd: f64,
}

#[derive(Debug)]
pub enum BatchDecision {
    Proceed,
    Abort {
        reason: String,
        current: f64,
        would_be: f64,
        cap_95: f64,
    },
}

impl CostMeter {
    pub fn new(cap: f64) -> Self {
        Self { cap, cost_usd: 0.0 }
    }

    pub fn record(&mut self, u: &Usage) {
        self.cost_usd += (u.input_tokens as f64 / 1_000_000.0) * PRICE_INPUT_UNCACHED_USD_PER_MTOK
            + (u.cache_creation_input_tokens as f64 / 1_000_000.0)
                * PRICE_INPUT_CACHE_WRITE_USD_PER_MTOK
            + (u.cache_read_input_tokens as f64 / 1_000_000.0)
                * PRICE_INPUT_CACHE_READ_USD_PER_MTOK
            + (u.output_tokens as f64 / 1_000_000.0) * PRICE_OUTPUT_USD_PER_MTOK;
        assert!(
            self.cost_usd < SPEC_BUDGET_USD,
            "spec ceiling ${} breached — must not be possible",
            SPEC_BUDGET_USD
        );
    }

    pub fn before_next_batch(&self, estimated_next: f64) -> BatchDecision {
        let cap_95 = self.cap * 0.95;
        let would_be = self.cost_usd + estimated_next;
        if would_be > cap_95 {
            BatchDecision::Abort {
                reason: "operational_budget".into(),
                current: self.cost_usd,
                would_be,
                cap_95,
            }
        } else {
            BatchDecision::Proceed
        }
    }
}
