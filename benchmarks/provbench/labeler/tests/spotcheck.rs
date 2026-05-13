use provbench_labeler::label::Label;
use provbench_labeler::output::OutputRow;
use provbench_labeler::spotcheck::{
    read_report_counts, sample, wilson_lower_bound_95, write_csv, DEFAULT_SEED,
};

#[test]
fn deterministic_sampler_returns_same_indices_across_runs() {
    let rows: Vec<OutputRow> = (0..1000)
        .map(|i| OutputRow {
            fact_id: format!("f{i}"),
            commit_sha: format!("c{}", i % 10),
            label: if i % 5 == 0 {
                Label::StaleSourceChanged
            } else {
                Label::Valid
            },
        })
        .collect();
    let s1 = sample(&rows, 200, DEFAULT_SEED);
    let s2 = sample(&rows, 200, DEFAULT_SEED);
    assert_eq!(s1.len(), 200);
    assert_eq!(s1, s2);
}

#[test]
fn rare_classes_meet_min_floor() {
    let rows: Vec<OutputRow> = (0..1000)
        .map(|i| OutputRow {
            fact_id: format!("f{i}"),
            commit_sha: format!("c{i}"),
            label: match i % 100 {
                0..=1 => Label::StaleSymbolRenamed {
                    new_name: "x".into(),
                },
                2..=3 => Label::StaleSourceDeleted,
                _ => Label::Valid,
            },
        })
        .collect();
    let s = sample(&rows, 200, DEFAULT_SEED);
    let renamed = s
        .iter()
        .filter(|r| matches!(r.row.label, Label::StaleSymbolRenamed { .. }))
        .count();
    assert!(renamed >= 10, "rare class under-sampled: got {renamed}");
}

#[test]
fn wilson_lower_bound_at_perfect_score() {
    let lb = wilson_lower_bound_95(200, 200);
    assert!(lb > 0.98, "got {lb}");
}

#[test]
fn wilson_lower_bound_at_95_point_estimate() {
    let lb = wilson_lower_bound_95(190, 200);
    // analytic: ~0.910
    assert!(lb > 0.90 && lb < 0.93, "got {lb}");
}

#[test]
fn wilson_lower_bound_at_perfect_score_is_above_correct_threshold() {
    let lb = wilson_lower_bound_95(199, 200);
    assert!((lb - 0.972_226_295_6).abs() < 0.000_05, "got {lb}");
}

#[test]
fn wilson_lower_bound_zero_total_returns_zero() {
    assert_eq!(wilson_lower_bound_95(0, 0), 0.0);
}

/// End-to-end integration regression for Task 5's `csv` crate adoption:
/// writes a real CSV file via the public `write_csv`, simulates a human
/// reviewer filling in `human_label` and `disagreement_notes` containing
/// every CSV-hostile character (`,`, `"`, `\n`, `\r`), then reads the
/// file back via the public `read_report_counts` and confirms the
/// reviewer row was parsed correctly without column drift.
///
/// This is the on-disk integration counterpart to the in-memory unit
/// tests in `src/spotcheck.rs::tests`: it proves the file written by
/// the labeler can be edited by a human reviewer using the same
/// CSV-quoting conventions and round-tripped through the public reader.
#[test]
fn write_csv_then_read_report_counts_round_trips_hostile_notes_on_disk() {
    // Two pre-classified rows, both predicted Valid, in two different
    // buckets so the sampler keeps both.
    let rows = vec![
        OutputRow {
            fact_id: "FunctionSignature::foo::src/lib.rs::1".to_string(),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_string(),
            label: Label::Valid,
        },
        OutputRow {
            fact_id: "Field::Config::limit::src/lib.rs::1".to_string(),
            commit_sha: "abcdef0123456789abcdef0123456789abcdef01".to_string(),
            label: Label::StaleSourceChanged,
        },
    ];
    let samples = sample(&rows, 2, DEFAULT_SEED);
    assert_eq!(
        samples.len(),
        2,
        "sampler returned wrong count: {samples:?}"
    );

    let tmp = tempfile::tempdir().unwrap();
    let csv_path = tmp.path().join("spotcheck.csv");
    write_csv(&csv_path, &samples).expect("write_csv must succeed");

    // Re-read the CSV file from disk, simulate a human reviewer filling
    // in `human_label` and `disagreement_notes` with every CSV-hostile
    // character, and write it back. We rely on the `csv` crate to do
    // the quoting correctly — that's exactly the round-trip Task 5 was
    // hardened to support.
    let raw = std::fs::read_to_string(&csv_path).expect("read original csv");
    let mut rdr = csv::Reader::from_reader(raw.as_bytes());
    let headers = rdr.headers().expect("csv must have headers").clone();
    let mut filled_rows: Vec<Vec<String>> = Vec::new();
    for record in rdr.records() {
        let rec = record.expect("read record");
        let mut row: Vec<String> = rec.iter().map(|s| s.to_string()).collect();
        // Columns: fact_id, commit_sha, bucket, predicted_label, human_label, disagreement_notes
        // Set human_label = predicted_label (an "agree" reviewer decision)
        // and disagreement_notes to a string with every hostile char.
        row[4] = row[3].clone();
        row[5] = "comma, quote \", LF\n, CR\r, all together".to_string();
        filled_rows.push(row);
    }

    {
        let mut wtr = csv::Writer::from_path(&csv_path).expect("rewrite csv");
        wtr.write_record(headers.iter()).unwrap();
        for row in &filled_rows {
            wtr.write_record(row).unwrap();
        }
        wtr.flush().unwrap();
    }

    // Public reader: must parse the on-disk CSV correctly despite the
    // hostile chars, producing 2 reviewed rows, all agreeing.
    let (agree, total) =
        read_report_counts(&csv_path).expect("read_report_counts must succeed on hostile notes");
    assert_eq!(total, 2, "expected 2 reviewed rows, got {total}");
    assert_eq!(agree, 2, "all 2 reviewer rows agreed; got {agree}");

    // Defence-in-depth: confirm the on-disk file actually contains the
    // hostile chars (otherwise the round-trip might be passing only
    // because the reviewer column is empty).
    let final_bytes = std::fs::read(&csv_path).unwrap();
    // Quoted CR/LF survive in the CSV body; the literal hostile chars
    // appear inside double-quoted cells — the csv crate is responsible
    // for choosing when to quote.
    let final_text = String::from_utf8(final_bytes).unwrap();
    assert!(
        final_text.contains('\"'),
        "expected at least one quoted cell in the round-tripped CSV: {final_text}"
    );
}
