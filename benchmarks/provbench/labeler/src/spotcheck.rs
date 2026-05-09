//! Stratified deterministic sampler for the spot-check process.
//! Seed is fixed (`0xC0DEBABE_DEADBEEF`) so re-running produces the same
//! CSV — important when the human reviewer fills it in over multiple
//! sessions.

use crate::output::OutputRow;
use rand::seq::SliceRandom;
use rand::SeedableRng;

const SEED: u64 = 0xC0DE_BABE_DEAD_BEEF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sampled {
    pub row: OutputRow,
    pub bucket: String,
}

pub fn sample(rows: &[OutputRow], n: usize) -> Vec<Sampled> {
    use std::collections::BTreeMap;
    let mut buckets: BTreeMap<String, Vec<&OutputRow>> = BTreeMap::new();
    for r in rows {
        buckets.entry(label_bucket(&r.label)).or_default().push(r);
    }
    let total = rows.len();
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(SEED);
    let mut out = Vec::new();
    let class_count = buckets.len().max(1);
    let per_class_floor = (n / (class_count * 2)).max(10).min(n);
    let mut deficit_pool: Vec<Sampled> = Vec::new();
    for (label, mut items) in buckets {
        items.shuffle(&mut rng);
        let proportional = ((items.len() as f64 / total as f64) * n as f64).round() as usize;
        let take = proportional.max(per_class_floor).min(items.len());
        for r in items.iter().take(take) {
            out.push(Sampled {
                row: (*r).clone(),
                bucket: label.clone(),
            });
        }
        for r in items.iter().skip(take) {
            deficit_pool.push(Sampled {
                row: (*r).clone(),
                bucket: label.clone(),
            });
        }
    }
    if out.len() > n {
        out.truncate(n);
    } else if out.len() < n {
        deficit_pool.shuffle(&mut rng);
        for s in deficit_pool.into_iter().take(n - out.len()) {
            out.push(s);
        }
    }
    out.sort_by(|a, b| {
        a.row
            .fact_id
            .cmp(&b.row.fact_id)
            .then_with(|| a.row.commit_sha.cmp(&b.row.commit_sha))
    });
    out
}

fn label_bucket(label: &crate::label::Label) -> String {
    use crate::label::Label::*;
    match label {
        Valid => "valid".into(),
        StaleSourceChanged => "stale_source_changed".into(),
        StaleSourceDeleted => "stale_source_deleted".into(),
        StaleSymbolRenamed { .. } => "stale_symbol_renamed".into(),
        NeedsRevalidation => "needs_revalidation".into(),
    }
}

pub fn write_csv(path: &std::path::Path, samples: &[Sampled]) -> anyhow::Result<()> {
    let mut f = std::fs::File::create(path)?;
    use std::io::Write;
    writeln!(
        f,
        "fact_id,commit_sha,bucket,predicted_label,human_label,disagreement_notes"
    )?;
    for s in samples {
        let predicted = label_bucket(&s.row.label);
        writeln!(
            f,
            "{},{},{},{},,",
            csv_escape(&s.row.fact_id),
            csv_escape(&s.row.commit_sha),
            csv_escape(&s.bucket),
            csv_escape(&predicted),
        )?;
    }
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
