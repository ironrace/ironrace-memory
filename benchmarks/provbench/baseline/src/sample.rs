//! Deterministic stratified sampler.
//!
//! Groups eligible `(fact_id, commit_sha, label_tag)` triples by their
//! coalesced [`StratumKey`], shuffles each pool with a ChaCha20 PRNG
//! seeded from `seed`, and takes the per-stratum target. The output
//! order is the stratum array's fixed order, not HashMap iteration
//! order — output is byte-deterministic for a fixed seed.

use crate::constants::*;
use rand::seq::SliceRandom;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerStratumTargets {
    pub valid: usize,
    pub stale_changed: usize,
    pub stale_deleted: usize,
    /// `usize::MAX` is the sentinel meaning "take the entire stratum".
    pub stale_renamed: usize,
    pub needs_revalidation: usize,
}

impl Default for PerStratumTargets {
    fn default() -> Self {
        Self {
            valid: TARGET_VALID,
            stale_changed: TARGET_STALE_CHANGED,
            stale_deleted: TARGET_STALE_DELETED,
            stale_renamed: TARGET_STALE_RENAMED,
            needs_revalidation: TARGET_NEEDS_REVALIDATION,
        }
    }
}

/// Coalesced label class used for stratification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StratumKey {
    Valid,
    StaleChanged,
    StaleDeleted,
    StaleRenamed,
    NeedsRevalidation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampledRow {
    pub fact_id: String,
    pub commit_sha: String,
    /// Serialized label tag (`"Valid"`, `"StaleSourceChanged"`,
    /// `"StaleSourceDeleted"`, `"StaleSymbolRenamed"`,
    /// `"NeedsRevalidation"`).
    pub ground_truth: String,
    pub stratum: StratumKey,
}

/// Group `rows` by stratum, sort each pool by `(fact_id, commit_sha)`,
/// shuffle with the seeded PRNG, then take the per-stratum target.
///
/// Determinism contract: output is byte-identical for fixed
/// `(rows, seed, targets)`. The HashMap used internally only stores
/// keyed pools — iteration order over output strata is a fixed array.
pub fn stratify_and_sample(
    rows: impl IntoIterator<Item = (String, String, String)>, // (fact_id, commit_sha, label_tag)
    seed: u64,
    targets: &PerStratumTargets,
) -> Vec<SampledRow> {
    let mut by_stratum: std::collections::HashMap<StratumKey, Vec<SampledRow>> = Default::default();
    for (fid, csh, lab) in rows {
        let stratum = match lab.as_str() {
            "Valid" => StratumKey::Valid,
            "StaleSourceChanged" => StratumKey::StaleChanged,
            "StaleSourceDeleted" => StratumKey::StaleDeleted,
            "StaleSymbolRenamed" => StratumKey::StaleRenamed,
            "NeedsRevalidation" => StratumKey::NeedsRevalidation,
            _ => continue,
        };
        by_stratum.entry(stratum).or_default().push(SampledRow {
            fact_id: fid,
            commit_sha: csh,
            ground_truth: lab,
            stratum,
        });
    }
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let mut out = Vec::new();
    for (stratum, target) in [
        (StratumKey::Valid, targets.valid),
        (StratumKey::StaleChanged, targets.stale_changed),
        (StratumKey::StaleDeleted, targets.stale_deleted),
        (StratumKey::StaleRenamed, targets.stale_renamed),
        (StratumKey::NeedsRevalidation, targets.needs_revalidation),
    ] {
        let mut pool = by_stratum.remove(&stratum).unwrap_or_default();
        pool.sort_by(|a, b| {
            a.fact_id
                .cmp(&b.fact_id)
                .then_with(|| a.commit_sha.cmp(&b.commit_sha))
        });
        pool.shuffle(&mut rng);
        let n = if target == usize::MAX {
            pool.len()
        } else {
            target.min(pool.len())
        };
        out.extend(pool.into_iter().take(n));
    }
    out
}
