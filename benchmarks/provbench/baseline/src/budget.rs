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
use std::collections::{BTreeMap, HashMap};

pub fn preflight_worst_case_cost(
    rows: &[SampledRow],
    diffs: &HashMap<String, DiffArtifact>,
    facts: &HashMap<String, FactBody>,
) -> f64 {
    let mut commits: BTreeMap<&String, Vec<&SampledRow>> = BTreeMap::new();
    for r in rows {
        commits.entry(&r.commit_sha).or_default().push(r);
    }

    let mut total_usd = 0.0;

    for (commit, group) in commits {
        let n_batches = group.len().div_ceil(MAX_FACTS_PER_BATCH);
        let diff_tokens = diff_token_estimate(diffs.get(commit));
        for (batch_index, chunk) in group.chunks(MAX_FACTS_PER_BATCH).enumerate() {
            let cacheable_tokens = STATIC_PREFIX_TOKENS + diff_tokens + FACTS_SEPARATOR_TOKENS;
            let facts_block_tokens = chunk
                .iter()
                .map(|row| fact_token_estimate(facts.get(&row.fact_id)))
                .sum::<f64>();
            let cacheable_price = if n_batches > 1 {
                if batch_index == 0 {
                    PRICE_INPUT_CACHE_WRITE_USD_PER_MTOK
                } else {
                    PRICE_INPUT_CACHE_READ_USD_PER_MTOK
                }
            } else {
                PRICE_INPUT_UNCACHED_USD_PER_MTOK
            };

            let input_usd = (cacheable_tokens / 1_000_000.0) * cacheable_price
                + (facts_block_tokens / 1_000_000.0) * PRICE_INPUT_UNCACHED_USD_PER_MTOK;
            let output_usd = (WORST_CASE_OUTPUT_TOKENS / 1_000_000.0) * PRICE_OUTPUT_USD_PER_MTOK;
            total_usd += input_usd + output_usd;
        }
    }
    total_usd
}

const STATIC_PREFIX_TOKENS: f64 = 250.0;
const FACTS_SEPARATOR_TOKENS: f64 = 10.0;
const WORST_CASE_OUTPUT_TOKENS: f64 = 1_800.0;
const INPUT_SIZE_SAFETY_MULTIPLIER: f64 = 1.5;

fn diff_token_estimate(diff: Option<&DiffArtifact>) -> f64 {
    match diff {
        Some(DiffArtifact::Included { unified_diff, .. }) => {
            (unified_diff.len() as f64 / 4.0) * INPUT_SIZE_SAFETY_MULTIPLIER
        }
        _ => 2_000.0,
    }
}

fn fact_token_estimate(fact: Option<&FactBody>) -> f64 {
    match fact {
        Some(f) => {
            ((f.body.len()
                + f.source_path.len()
                + f.symbol_path.len()
                + f.content_hash_at_observation.len()
                + 80) as f64
                / 4.0)
                * INPUT_SIZE_SAFETY_MULTIPLIER
        }
        None => 80.0,
    }
}

#[derive(Debug, Clone)]
pub struct CostMeter {
    pub cap: f64,
    pub cost_usd: f64,
    pub tokens_in_uncached: u64,
    pub tokens_in_cache_write: u64,
    pub tokens_in_cache_read: u64,
    pub tokens_out: u64,
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
        Self {
            cap,
            cost_usd: 0.0,
            tokens_in_uncached: 0,
            tokens_in_cache_write: 0,
            tokens_in_cache_read: 0,
            tokens_out: 0,
        }
    }

    /// Record a single API call's `Usage`, accumulating its USD cost.
    ///
    /// Returns `Err` (rather than panicking) when the running total
    /// reaches the SPEC §6.2 / §15 immutable ceiling. The caller is
    /// expected to abort the run, persist `run_meta.json`, and exit
    /// non-zero so CI surfaces the breach. The operational `cap`
    /// (≤ ceiling) is enforced separately by
    /// [`CostMeter::before_next_batch`] as a pre-dispatch gate.
    pub fn record(&mut self, u: &Usage) -> anyhow::Result<()> {
        self.tokens_in_uncached += u.input_tokens as u64;
        self.tokens_in_cache_write += u.cache_creation_input_tokens as u64;
        self.tokens_in_cache_read += u.cache_read_input_tokens as u64;
        self.tokens_out += u.output_tokens as u64;
        self.cost_usd += (u.input_tokens as f64 / 1_000_000.0) * PRICE_INPUT_UNCACHED_USD_PER_MTOK
            + (u.cache_creation_input_tokens as f64 / 1_000_000.0)
                * PRICE_INPUT_CACHE_WRITE_USD_PER_MTOK
            + (u.cache_read_input_tokens as f64 / 1_000_000.0)
                * PRICE_INPUT_CACHE_READ_USD_PER_MTOK
            + (u.output_tokens as f64 / 1_000_000.0) * PRICE_OUTPUT_USD_PER_MTOK;
        anyhow::ensure!(
            self.cost_usd < SPEC_BUDGET_USD,
            "spec ceiling ${} breached: cost_usd={}",
            SPEC_BUDGET_USD,
            self.cost_usd
        );
        Ok(())
    }

    pub fn total_tokens(&self) -> u64 {
        self.tokens_in_uncached
            + self.tokens_in_cache_write
            + self.tokens_in_cache_read
            + self.tokens_out
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
