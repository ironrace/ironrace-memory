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
/// stratified sampler has something to work with. fact_id paths
/// alternate between `.rs` and `.py` extensions so the `--lang` filter
/// has both languages to choose from. Returns the file path inside
/// `tmp`.
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
        let ext = if i % 2 == 0 { "rs" } else { "py" };
        lines.push(format!(
            r#"{{"fact_id":"DocClaim::auto::path/file-{i:04}.{ext}::{i}","commit_sha":"sha-{:08x}","label":{lbl},"labeler_git_sha":"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"}}"#,
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

/// `--lang python` must restrict the emitted CSV to rows whose
/// fact_id path ends in `.py`. The synthetic corpus is half Rust /
/// half Python, so the filter must drop every `.rs` row before
/// sampling.
#[test]
fn spotcheck_cli_lang_python_filters_to_py_only() {
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
        .arg("--lang")
        .arg("python")
        .status()
        .expect("spawn provbench-labeler");
    assert!(status.success(), "spotcheck exited non-zero: {status:?}");

    let csv = std::fs::read_to_string(&out).expect("read csv");
    let mut data_rows = 0;
    for (idx, line) in csv.lines().enumerate() {
        if idx == 0 {
            // header
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let fact_id = line.split(',').next().expect("first column");
        assert!(
            fact_id.contains(".py::"),
            "non-python row leaked through --lang python filter: {fact_id}"
        );
        assert!(
            !fact_id.contains(".rs::"),
            "rust row leaked through --lang python filter: {fact_id}"
        );
        data_rows += 1;
    }
    assert!(data_rows > 0, "expected at least one python row in CSV");
}

/// `--lang rust` mirrors the python case, restricting to `.rs` only.
#[test]
fn spotcheck_cli_lang_rust_filters_to_rs_only() {
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
        .arg("--lang")
        .arg("rust")
        .status()
        .expect("spawn provbench-labeler");
    assert!(status.success(), "spotcheck exited non-zero: {status:?}");

    let csv = std::fs::read_to_string(&out).expect("read csv");
    let mut data_rows = 0;
    for (idx, line) in csv.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let fact_id = line.split(',').next().expect("first column");
        assert!(
            fact_id.contains(".rs::"),
            "non-rust row leaked through --lang rust filter: {fact_id}"
        );
        data_rows += 1;
    }
    assert!(data_rows > 0, "expected at least one rust row in CSV");
}

/// Back-compat regression: omitting `--lang` (default = `both`)
/// produces the same CSV bytes as explicitly passing `--lang both`.
/// This is the load-bearing invariant promised by Task 14 — pre-flag
/// callers must see no behavior change.
#[test]
fn spotcheck_cli_lang_default_equals_lang_both_bytes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let corpus = write_synthetic_corpus(tmp.path());
    let bin = env!("CARGO_BIN_EXE_provbench-labeler");

    let run = |out: &std::path::Path, lang_args: &[&str]| {
        let mut cmd = Command::new(bin);
        cmd.arg("spotcheck")
            .arg("--corpus")
            .arg(&corpus)
            .arg("--out")
            .arg(out)
            .arg("--n")
            .arg("20")
            .arg("--seed")
            .arg("0xC0DEBABEDEADBEEF");
        for a in lang_args {
            cmd.arg(a);
        }
        let status = cmd.status().expect("spawn provbench-labeler");
        assert!(status.success(), "spotcheck exited non-zero: {status:?}");
    };

    let out_default = tmp.path().join("default.csv");
    let out_both = tmp.path().join("both.csv");
    run(&out_default, &[]);
    run(&out_both, &["--lang", "both"]);

    let bytes_default = std::fs::read(&out_default).expect("read default csv");
    let bytes_both = std::fs::read(&out_both).expect("read both csv");
    assert_eq!(
        bytes_default, bytes_both,
        "default CSV must be byte-identical to --lang both"
    );
}
