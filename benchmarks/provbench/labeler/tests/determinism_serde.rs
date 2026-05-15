//! SPEC §9.4 held-out determinism gate for serde-rs/serde @ 65e1a507.
//!
//! Runs the labeler pipeline twice over `benchmarks/provbench/work/serde`
//! and asserts byte-identical corpus, facts, and diff artifacts. This is
//! ignored by default because the held-out serde replay is intentionally
//! large.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const SERDE_T0: &str = "65e1a50749938612cfbdb69b57fc4cf249f87149";

struct LabelerOutputs {
    corpus: PathBuf,
    facts: PathBuf,
    diffs_dir: PathBuf,
}

fn provbench_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn labeler_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_provbench-labeler"))
}

fn run_labeler(args: &[&str]) {
    let status = Command::new(labeler_bin())
        .args(args)
        .status()
        .expect("spawn provbench-labeler");
    assert!(status.success(), "provbench-labeler {args:?} failed");
}

fn run_pipeline(work_serde: &Path, out_dir: &Path) -> LabelerOutputs {
    let corpus = out_dir.join("serde-65e1a507-c2d3b7b.jsonl");
    let facts = out_dir.join("serde-65e1a507-c2d3b7b.facts.jsonl");
    let diffs_dir = out_dir.join("serde-65e1a507-c2d3b7b.diffs");

    run_labeler(&[
        "run",
        "--repo",
        work_serde.to_str().unwrap(),
        "--t0",
        SERDE_T0,
        "--out",
        corpus.to_str().unwrap(),
    ]);
    run_labeler(&[
        "emit-facts",
        "--corpus",
        corpus.to_str().unwrap(),
        "--repo",
        work_serde.to_str().unwrap(),
        "--t0",
        SERDE_T0,
        "--out",
        facts.to_str().unwrap(),
    ]);
    run_labeler(&[
        "emit-diffs",
        "--corpus",
        corpus.to_str().unwrap(),
        "--repo",
        work_serde.to_str().unwrap(),
        "--t0",
        SERDE_T0,
        "--out-dir",
        diffs_dir.to_str().unwrap(),
    ]);

    LabelerOutputs {
        corpus,
        facts,
        diffs_dir,
    }
}

fn read_diff_artifacts(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut artifacts = BTreeMap::new();
    for entry in std::fs::read_dir(dir).expect("read diffs dir") {
        let entry = entry.expect("read diff artifact entry");
        let path = entry.path();
        if path.is_file() {
            let name = path
                .file_name()
                .expect("diff artifact filename")
                .to_string_lossy()
                .into_owned();
            artifacts.insert(name, std::fs::read(path).expect("read diff artifact"));
        }
    }
    artifacts
}

#[test]
#[ignore = "requires benchmarks/provbench/work/serde checkout; expensive held-out replay"]
fn serde_held_out_outputs_are_byte_identical_across_runs() {
    let work_serde = provbench_root().join("work/serde");
    assert!(
        work_serde.exists(),
        "needs benchmarks/provbench/work/serde checkout for held-out determinism gate"
    );

    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let out_a = run_pipeline(&work_serde, a.path());
    let out_b = run_pipeline(&work_serde, b.path());

    assert_eq!(
        std::fs::read(out_a.corpus).expect("read corpus A"),
        std::fs::read(out_b.corpus).expect("read corpus B"),
        "serde held-out corpus output is non-deterministic"
    );
    assert_eq!(
        std::fs::read(out_a.facts).expect("read facts A"),
        std::fs::read(out_b.facts).expect("read facts B"),
        "serde held-out facts output is non-deterministic"
    );
    assert_eq!(
        read_diff_artifacts(&out_a.diffs_dir),
        read_diff_artifacts(&out_b.diffs_dir),
        "serde held-out diff artifacts are non-deterministic"
    );
}
