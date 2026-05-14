//! `SampleManifest` — canonical, content-hashed record of a stratified
//! draw from a labeler corpus.
//!
//! Determinism contract: for fixed `(corpus, facts, diffs, seed,
//! targets, budget)`, [`SampleManifest::from_corpus`] returns a struct
//! whose `canonical_json()` is byte-identical across runs **on the same
//! git HEAD**. `baseline_crate_head_sha` is intentionally part of the
//! content hash — it's provenance, not flake.
//!
//! Excluded rows are recorded in `excluded_count_by_reason: BTreeMap`
//! (sorted iteration → deterministic JSON) — never silently dropped.

use crate::diffs::{load_diffs_dir, DiffArtifact};
use crate::facts::load_facts;
use crate::sample::{stratify_and_sample, PerStratumTargets, SampledRow};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// Path to the frozen SPEC file, resolved relative to this crate's
/// manifest dir. Stable across CWDs and machines (the crate is the
/// anchor, not the invocation site).
const SPEC_MD_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../SPEC.md");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleManifest {
    pub seed: u64,
    pub corpus_path: PathBuf,
    pub facts_path: PathBuf,
    pub diffs_dir: PathBuf,
    pub labeler_git_sha: String,
    pub spec_freeze_hash: String,
    pub baseline_crate_head_sha: String,
    pub per_stratum_targets: PerStratumTargets,
    pub selected_count: usize,
    /// Reason → count. `BTreeMap` chosen so serde_json emits sorted
    /// keys (HashMap iteration order is undefined and would break the
    /// content-hash determinism contract).
    pub excluded_count_by_reason: BTreeMap<String, usize>,
    pub estimated_worst_case_usd: f64,
    pub rows: Vec<SampledRow>,
    pub created_at: String,
    /// `sha256` of the canonical JSON with this field blanked.
    pub content_hash: String,
}

impl SampleManifest {
    pub fn from_corpus(
        corpus_path: &Path,
        facts_path: &Path,
        diffs_dir: &Path,
        seed: u64,
        targets: PerStratumTargets,
        budget_usd: f64,
    ) -> Result<Self> {
        let facts = load_facts(facts_path)?;
        let diffs = load_diffs_dir(diffs_dir)?;
        let mut excluded: BTreeMap<String, usize> = BTreeMap::new();
        let mut eligible: Vec<(String, String, String)> = Vec::new();
        let mut labeler_git_sha: String = String::new();

        let f = std::fs::File::open(corpus_path)
            .with_context(|| format!("open corpus {}", corpus_path.display()))?;
        for line in std::io::BufReader::new(f).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => {
                    *excluded.entry("malformed_row".into()).or_insert(0) += 1;
                    continue;
                }
            };
            let fact_id = v["fact_id"].as_str().unwrap_or_default().to_string();
            let commit_sha = v["commit_sha"].as_str().unwrap_or_default().to_string();
            let label_tag = match &v["label"] {
                serde_json::Value::String(s) => s.clone(),
                // `StaleSymbolRenamed` lands here: the first (and only) key
                // is the variant name (e.g. `"StaleSymbolRenamed"`).
                serde_json::Value::Object(map) => map.keys().next().cloned().unwrap_or_default(),
                _ => {
                    *excluded.entry("malformed_label".into()).or_insert(0) += 1;
                    continue;
                }
            };
            if labeler_git_sha.is_empty() {
                if let Some(s) = v["labeler_git_sha"].as_str() {
                    labeler_git_sha = s.to_string();
                }
            }
            if !facts.contains_key(&fact_id) {
                *excluded.entry("missing_fact_body".into()).or_insert(0) += 1;
                continue;
            }
            match diffs.get(&commit_sha) {
                Some(DiffArtifact::Included { .. }) => {}
                Some(DiffArtifact::Excluded {
                    excluded: reason, ..
                }) => {
                    *excluded.entry(format!("commit_{reason}")).or_insert(0) += 1;
                    continue;
                }
                None => {
                    *excluded.entry("missing_diff_artifact".into()).or_insert(0) += 1;
                    continue;
                }
            }
            eligible.push((fact_id, commit_sha, label_tag));
        }

        let rows = stratify_and_sample(eligible, seed, &targets);
        let selected_count = rows.len();
        let estimated_worst_case_usd =
            crate::budget::preflight_worst_case_cost(&rows, &diffs, &facts);

        anyhow::ensure!(
            estimated_worst_case_usd <= budget_usd,
            "preflight refuses: estimated worst-case ${:.2} > budget ${:.2}",
            estimated_worst_case_usd,
            budget_usd
        );

        let spec_freeze_hash = compute_sha256_of_path(Path::new(SPEC_MD_PATH))
            .with_context(|| format!("hash SPEC.md at {SPEC_MD_PATH}"))?;
        let baseline_crate_head_sha = git_head_sha_or_unknown();
        let created_at = chrono_like_utc_now();

        let mut m = SampleManifest {
            seed,
            corpus_path: corpus_path.to_path_buf(),
            facts_path: facts_path.to_path_buf(),
            diffs_dir: diffs_dir.to_path_buf(),
            labeler_git_sha,
            spec_freeze_hash,
            baseline_crate_head_sha,
            per_stratum_targets: targets,
            selected_count,
            excluded_count_by_reason: excluded,
            estimated_worst_case_usd,
            rows,
            created_at,
            content_hash: String::new(),
        };
        m.content_hash = m.compute_content_hash();
        Ok(m)
    }

    /// Canonical JSON form used for hashing and equality.
    ///
    /// Field order follows the struct declaration order (serde default).
    /// Maps are `BTreeMap` so their entries are emitted in sorted-key
    /// order — both contribute to byte-determinism.
    pub fn canonical_json(&self) -> String {
        serde_json::to_string(self).expect("SampleManifest serialization must not fail")
    }

    fn compute_content_hash(&self) -> String {
        let mut tmp = self.clone();
        tmp.content_hash = String::new();
        let mut hasher = Sha256::new();
        hasher
            .update(serde_json::to_vec(&tmp).expect("SampleManifest serialization must not fail"));
        hex::encode(hasher.finalize())
    }

    /// Atomically persist this manifest to `path` via tmp + rename.
    pub fn save_atomic(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest path has no parent: {}", path.display()))?;
        std::fs::create_dir_all(parent)?;
        let tmp = parent.join(format!(".manifest.tmp.{}", std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

fn compute_sha256_of_path(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read for hash: {}", path.display()))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

fn git_head_sha_or_unknown() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn chrono_like_utc_now() -> String {
    // Avoid a chrono dependency — record provenance via Unix epoch
    // seconds. Shape is stable across machines.
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix-seconds-{s}Z")
}
