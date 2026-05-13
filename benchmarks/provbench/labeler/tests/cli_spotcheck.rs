//! End-to-end CLI regression for `provbench-labeler spotcheck`.
//!
//! Unit tests in `tests/spotcheck.rs` exercise the public sampler and
//! the sidecar writer in isolation, but they would still pass if a
//! refactor accidentally dropped the `write_meta_sidecar` call from
//! the CLI wiring in `main.rs`. This test invokes the compiled binary
//! against a tiny synthetic JSONL corpus and asserts that the full
//! pipeline — argument parsing, CSV write, sidecar write — produces
//! the expected on-disk artifacts.

use std::process::Command;

use provbench_labeler::spotcheck::SpotCheckMeta;

/// Build a 30-row JSONL corpus spanning every label bucket so the
/// stratified sampler has something to work with. Returns the file
/// path inside `tmp`.
fn write_synthetic_corpus(tmp: &std::path::Path) -> std::path::PathBuf {
    let corpus = tmp.join("corpus.jsonl");
    let mut lines = Vec::new();
    let labels = [
        r#""Valid""#,
        r#""StaleSourceChanged""#,
        r#""StaleSourceDeleted""#,
        r#"{"StaleSymbolRenamed":{"new_name":"x"}}"#,
        r#""NeedsRevalidation""#,
    ];
    for (i, lbl) in (0..30).zip(labels.iter().cycle()) {
        lines.push(format!(
            r#"{{"fact_id":"fact-{i:04}","commit_sha":"sha-{:08x}","label":{lbl},"labeler_git_sha":"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"}}"#,
            i * 7
        ));
    }
    std::fs::write(&corpus, lines.join("\n") + "\n").expect("write corpus jsonl");
    corpus
}

/// Running `provbench-labeler spotcheck --seed 0x...` against a small
/// synthetic corpus must produce both the CSV at `--out` and the
/// `<out>.meta.json` sidecar, and the sidecar must record the resolved
/// seed, the requested `n`, and the corpus path verbatim.
#[test]
fn spotcheck_cli_writes_csv_and_meta_sidecar() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let corpus = write_synthetic_corpus(tmp.path());
    let out = tmp.path().join("sample.csv");
    let sidecar = tmp.path().join("sample.csv.meta.json");

    let bin = env!("CARGO_BIN_EXE_provbench-labeler");
    let status = Command::new(bin)
        .arg("spotcheck")
        .arg("--corpus")
        .arg(&corpus)
        .arg("--out")
        .arg(&out)
        .arg("--n")
        .arg("20")
        .arg("--seed")
        .arg("0xDEADBEEFCAFEF00D")
        .status()
        .expect("spawn provbench-labeler");
    assert!(status.success(), "spotcheck exited non-zero: {status:?}");

    assert!(out.exists(), "CSV must exist at {}", out.display());
    assert!(
        sidecar.exists(),
        "sidecar must exist at {} — did write_meta_sidecar get dropped from the CLI wiring?",
        sidecar.display()
    );

    let bytes = std::fs::read(&sidecar).expect("read sidecar");
    let meta: SpotCheckMeta = serde_json::from_slice(&bytes).expect("parse sidecar");
    assert_eq!(meta.seed, 0xDEAD_BEEF_CAFE_F00D);
    assert_eq!(meta.n, 20);
    assert_eq!(
        meta.corpus,
        corpus.display().to_string(),
        "sidecar must record the corpus path verbatim"
    );
    assert!(
        !meta.labeler_git_sha.is_empty(),
        "labeler_git_sha must be populated (got empty string)"
    );
}

/// Decimal seeds must round-trip through the CLI into the sidecar as
/// the same numeric value. Guards against a future refactor that
/// changes the parser to reject one of the two accepted forms.
#[test]
fn spotcheck_cli_accepts_decimal_seed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let corpus = write_synthetic_corpus(tmp.path());
    let out = tmp.path().join("sample.csv");

    let bin = env!("CARGO_BIN_EXE_provbench-labeler");
    let status = Command::new(bin)
        .arg("spotcheck")
        .arg("--corpus")
        .arg(&corpus)
        .arg("--out")
        .arg(&out)
        .arg("--n")
        .arg("20")
        .arg("--seed")
        .arg("12345")
        .status()
        .expect("spawn provbench-labeler");
    assert!(status.success(), "spotcheck exited non-zero: {status:?}");

    let bytes = std::fs::read(out.with_extension("csv.meta.json")).expect("read sidecar");
    let meta: SpotCheckMeta = serde_json::from_slice(&bytes).expect("parse sidecar");
    assert_eq!(meta.seed, 12345);
}
