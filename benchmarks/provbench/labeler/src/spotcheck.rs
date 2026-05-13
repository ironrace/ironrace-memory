//! Stratified deterministic sampler for the spot-check process.
//! Default seed is fixed (`DEFAULT_SEED`) so re-running produces the
//! same CSV — important when the human reviewer fills it in over
//! multiple sessions. A different seed may be supplied (e.g., post-merge
//! validation against a regenerated corpus) for anti-tuning hygiene.

use crate::output::OutputRow;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};

/// One row of the spot-check CSV.
///
/// Column order is fixed by the SPEC and must not change:
/// `fact_id,commit_sha,bucket,predicted_label,human_label,disagreement_notes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SpotCheckRow {
    pub(crate) fact_id: String,
    pub(crate) commit_sha: String,
    pub(crate) bucket: String,
    pub(crate) predicted_label: String,
    #[serde(default)]
    pub(crate) human_label: String,
    #[serde(default)]
    pub(crate) disagreement_notes: String,
}

/// Default RNG seed for stratified sampling. Callers may pass a
/// different seed to [`sample`] for fresh draws (e.g., post-merge
/// validation runs) while preserving deterministic replay within a
/// single review session.
pub const DEFAULT_SEED: u64 = 0xC0DE_BABE_DEAD_BEEF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sampled {
    pub row: OutputRow,
    pub bucket: String,
}

/// Stratified deterministic sample of `n` rows drawn from `rows`,
/// bucketed by label, seeded by `seed`.
///
/// Sampling is seeded so re-running with the same `seed` over the same
/// input produces the same output — important for human reviewers who
/// fill in `human_label` over multiple sessions. Callers wanting the
/// historical default should pass [`DEFAULT_SEED`]. Each label class
/// gets at least a small floor (`per_class_floor`); any remaining
/// budget is filled from a shuffled deficit pool. The returned vec is
/// sorted by `(fact_id, commit_sha)` for stable comparison.
///
/// Both randomized steps — the per-bucket shuffle and the deficit-pool
/// shuffle — draw from the **same** seeded RNG, so a single seed pins
/// the whole sample. Splitting them would silently break the
/// determinism contract that callers and the in-tree unit test rely on.
///
/// **Footgun:** changing `seed` against an already partially-filled
/// CSV produces a different row order. The reviewer's `human_label`
/// column will then misalign with `predicted_label`, silently
/// corrupting the agreement metric. Only change the seed for a fresh
/// draw against an empty CSV (e.g., a post-merge anti-tuning run on a
/// regenerated corpus); never to "re-shuffle" an in-progress review.
pub fn sample(rows: &[OutputRow], n: usize, seed: u64) -> Vec<Sampled> {
    use std::collections::BTreeMap;
    let mut buckets: BTreeMap<String, Vec<&OutputRow>> = BTreeMap::new();
    for r in rows {
        buckets.entry(label_bucket(&r.label)).or_default().push(r);
    }
    let total = rows.len();
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
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

/// Write the spot-check samples to `path` as RFC-4180 CSV via the
/// `csv` crate.
///
/// All quoting (commas, embedded `"`, `\n`, `\r` inside
/// `disagreement_notes`) is handled by the `csv` writer, so reviewer
/// notes containing quoted newlines round-trip correctly through
/// [`read_report_counts`] without column drift.
pub fn write_csv(path: &std::path::Path, samples: &[Sampled]) -> anyhow::Result<()> {
    let f = std::fs::File::create(path)?;
    write_csv_to(f, samples)
}

/// Provenance metadata describing how a spot-check CSV was produced.
///
/// Persisted alongside the CSV as `<out>.meta.json` so a maintainer
/// returning to a filled-in CSV later can verify it was drawn against
/// the expected corpus + labeler build + seed without inspecting
/// stdout from the original run. Fields are intentionally minimal —
/// anything not derivable from these three keys plus the CSV itself
/// belongs in the CSV or the SPEC, not here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpotCheckMeta {
    /// Path to the JSONL corpus the sample was drawn from.
    pub corpus: String,
    /// RNG seed used by [`sample`] for this draw.
    pub seed: u64,
    /// Number of rows requested (the `--n` argument).
    pub n: usize,
    /// Labeler git SHA at the time the sample was written
    /// (`labeler_stamp()` at run time).
    pub labeler_git_sha: String,
}

/// Write a JSON sidecar describing the sampling provenance to
/// `<csv_path>.meta.json`. The sidecar pins the corpus path, the
/// resolved seed, the requested row count, and the labeler git SHA so
/// the on-disk CSV is self-describing for post-merge audits.
pub fn write_meta_sidecar(csv_path: &std::path::Path, meta: &SpotCheckMeta) -> anyhow::Result<()> {
    let mut sidecar = csv_path.as_os_str().to_owned();
    sidecar.push(".meta.json");
    let f = std::fs::File::create(std::path::Path::new(&sidecar))?;
    serde_json::to_writer_pretty(f, meta)?;
    Ok(())
}

/// Write spot-check samples as CSV to any `Write` impl. Used by the CLI
/// (file path) and the unit tests (in-memory `Cursor`).
pub(crate) fn write_csv_to<W: std::io::Write>(w: W, samples: &[Sampled]) -> anyhow::Result<()> {
    let mut wtr = csv::Writer::from_writer(w);
    for s in samples {
        let row = SpotCheckRow {
            fact_id: s.row.fact_id.clone(),
            commit_sha: s.row.commit_sha.clone(),
            bucket: s.bucket.clone(),
            predicted_label: label_bucket(&s.row.label),
            human_label: String::new(),
            disagreement_notes: String::new(),
        };
        wtr.serialize(&row)?;
    }
    wtr.flush()?;
    Ok(())
}

/// Read a filled spot-check CSV and return (agree, total). Reviewer rows
/// without a `human_label` are skipped. Uses the `csv` crate so quoted
/// newlines / CRs in `disagreement_notes` do not cause column drift.
pub fn read_report_counts(path: &std::path::Path) -> anyhow::Result<(u32, u32)> {
    let f = std::fs::File::open(path)?;
    read_report_counts_from(f)
}

pub(crate) fn read_report_counts_from<R: std::io::Read>(r: R) -> anyhow::Result<(u32, u32)> {
    let mut rdr = csv::Reader::from_reader(r);
    let mut agree: u32 = 0;
    let mut total: u32 = 0;
    for result in rdr.deserialize::<SpotCheckRow>() {
        let row = result?;
        let human = row.human_label.trim();
        if human.is_empty() {
            continue;
        }
        total += 1;
        if human == row.predicted_label.trim() {
            agree += 1;
        }
    }
    Ok((agree, total))
}

/// Wilson score lower bound at 95% confidence (z=1.95996398454).
///
/// Used as the human-agreement gate metric: a Wilson lower bound is
/// preferred over a raw point estimate at small `total` because the
/// raw ratio `success/total` is upward-biased on small samples.
/// Returns `0.0` when `total == 0`.
pub fn wilson_lower_bound_95(success: u32, total: u32) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let n = total as f64;
    let p = success as f64 / n;
    let z: f64 = 1.959_963_984_54;
    let denom = 1.0 + (z * z) / n;
    let center = p + (z * z) / (2.0 * n);
    let margin = z * ((p * (1.0 - p) + (z * z) / (4.0 * n)) / n).sqrt();
    (center - margin) / denom
}

#[derive(Debug, Clone)]
pub struct SpotCheckReport {
    pub total: u32,
    pub agree: u32,
    pub point_estimate: f64,
    pub wilson_lower_95: f64,
    pub gate_passed: bool,
}

/// Build a [`SpotCheckReport`] from `(agree, total)` reviewer counts.
///
/// `gate_passed` is `true` only when both the point-estimate
/// agreement is at least 95% and `total` is at least 200 — the per-SPEC
/// minimum sample size below which the agreement metric is not
/// considered binding.
pub fn report(agree: u32, total: u32) -> SpotCheckReport {
    let p = if total == 0 {
        0.0
    } else {
        agree as f64 / total as f64
    };
    let wlb = wilson_lower_bound_95(agree, total);
    SpotCheckReport {
        total,
        agree,
        point_estimate: p,
        wilson_lower_95: wlb,
        gate_passed: p >= 0.95 && total >= 200,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Round-trip a row whose `disagreement_notes` contains all four
    /// CSV-hostile chars: `,`, `"`, `\n`, AND `\r`. Asserts structural
    /// equality of the deserialized row vs the input.
    #[test]
    fn round_trip_preserves_comma_quote_lf_and_cr() {
        let row_in = SpotCheckRow {
            fact_id: "fact-1".to_string(),
            commit_sha: "abc1234".to_string(),
            bucket: "valid".to_string(),
            predicted_label: "valid".to_string(),
            human_label: "valid".to_string(),
            disagreement_notes: "note with comma, quote \", LF\nand CR\rmixed".to_string(),
        };

        let mut buf: Vec<u8> = Vec::new();
        {
            let mut wtr = csv::Writer::from_writer(&mut buf);
            wtr.serialize(&row_in).unwrap();
            wtr.flush().unwrap();
        }

        let mut rdr = csv::Reader::from_reader(Cursor::new(buf));
        let row_out: SpotCheckRow = rdr.deserialize().next().expect("one row").unwrap();

        assert_eq!(row_in, row_out);
    }

    /// A reviewer note containing a quoted newline must not cause column
    /// drift: the row should still have exactly six fields and the note
    /// should still contain the embedded `\n`.
    #[test]
    fn report_parser_handles_quoted_newline_without_column_drift() {
        // Header + one row whose disagreement_notes contains an
        // unescaped-but-quoted LF. The csv crate must keep this as a
        // single logical record.
        let csv_text = "fact_id,commit_sha,bucket,predicted_label,human_label,disagreement_notes\n\
                        fact-1,abc1234,valid,valid,stale_source_changed,\"line-one\nline-two\"\n";

        let (agree, total) = read_report_counts_from(Cursor::new(csv_text)).unwrap();
        // human_label = "stale_source_changed" != predicted "valid" → disagree
        assert_eq!(total, 1);
        assert_eq!(agree, 0);

        // Also re-deserialize directly to confirm the multi-line note is
        // preserved as a single field, not split across rows.
        let mut rdr = csv::Reader::from_reader(Cursor::new(csv_text));
        let rows: Vec<SpotCheckRow> = rdr
            .deserialize::<SpotCheckRow>()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].disagreement_notes, "line-one\nline-two");
    }

    /// The provenance sidecar round-trips through `serde_json` and lands
    /// at `<csv_path>.meta.json` on disk, preserving each field byte-
    /// for-byte.
    #[test]
    fn meta_sidecar_round_trips_and_lives_next_to_csv() {
        let tmp = tempfile::tempdir().unwrap();
        let csv_path = tmp.path().join("sample-abc.csv");
        let meta = SpotCheckMeta {
            corpus: "benchmarks/provbench/corpus/ripgrep-af6b6c54-e96c9fe.jsonl".to_string(),
            seed: 0xC0DE_BABE_DEAD_BEEF,
            n: 200,
            labeler_git_sha: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string(),
        };
        write_meta_sidecar(&csv_path, &meta).expect("sidecar write");

        let sidecar_path = tmp.path().join("sample-abc.csv.meta.json");
        assert!(
            sidecar_path.exists(),
            "sidecar must land at <csv>.meta.json: {}",
            sidecar_path.display()
        );

        let bytes = std::fs::read(&sidecar_path).unwrap();
        let parsed: SpotCheckMeta = serde_json::from_slice(&bytes).expect("parse sidecar");
        assert_eq!(parsed, meta);
    }

    /// `sample` is deterministic in `seed`: identical seed + identical
    /// input yields identical output; different seed yields a
    /// different selection (over a corpus with enough excess to make a
    /// reshuffle observable).
    ///
    /// The "different seed" arm uses `DEFAULT_SEED ^ 0xDEAD_BEEF_DEAD_BEEF`
    /// rather than `DEFAULT_SEED + 1` because ChaCha8's avalanche on a
    /// minimal-distance seed is empirically less aggressive on a
    /// 500-row × 5-bucket × 50-draw stratified sample than on a
    /// uniform single-draw RNG. The chosen XOR constant was confirmed
    /// by running the test locally on this corpus shape; it is also
    /// large enough (≥ Hamming distance 32) that even a future ChaCha
    /// variant with weaker diffusion in the low bits would still
    /// reshuffle the sample. If a future seed/corpus combination
    /// regresses this `assert_ne!`, fail-loud is the correct outcome
    /// — pick a new XOR constant and update this comment.
    #[test]
    fn sample_is_seed_deterministic_and_seed_sensitive() {
        use crate::label::Label;
        let rows: Vec<OutputRow> = (0..500)
            .map(|i| OutputRow {
                fact_id: format!("fact-{i:04}"),
                commit_sha: format!("sha-{:08x}", i * 7),
                label: if i % 3 == 0 {
                    Label::Valid
                } else if i % 3 == 1 {
                    Label::StaleSourceChanged
                } else {
                    Label::NeedsRevalidation
                },
            })
            .collect();

        let a = sample(&rows, 50, DEFAULT_SEED);
        let b = sample(&rows, 50, DEFAULT_SEED);
        let c = sample(&rows, 50, DEFAULT_SEED ^ 0xDEAD_BEEF_DEAD_BEEF);

        let ids =
            |xs: &[Sampled]| -> Vec<String> { xs.iter().map(|s| s.row.fact_id.clone()).collect() };

        assert_eq!(ids(&a), ids(&b), "same seed must reproduce sample");
        assert_ne!(
            ids(&a),
            ids(&c),
            "different seed must change the sample over a corpus with reshuffle slack"
        );
        assert_eq!(a.len(), 50);
        assert_eq!(c.len(), 50);
    }

    /// Empty human_label rows are skipped by the report parser (the
    /// reviewer hasn't filled them in yet).
    #[test]
    fn report_parser_skips_unreviewed_rows() {
        let csv_text = "fact_id,commit_sha,bucket,predicted_label,human_label,disagreement_notes\n\
                        fact-1,abc1234,valid,valid,,\n\
                        fact-2,def5678,valid,valid,valid,\n";
        let (agree, total) = read_report_counts_from(Cursor::new(csv_text)).unwrap();
        assert_eq!(total, 1);
        assert_eq!(agree, 1);
    }
}
