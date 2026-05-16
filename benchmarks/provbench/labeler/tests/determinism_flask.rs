//! Plan A Task 17 — Python labeler determinism on the held-out flask checkout.
//!
//! Mirrors `determinism_python.rs` but points at the real
//! `benchmarks/provbench/work/flask` checkout (pinned at T₀
//! `2f0c62f5e6e290843f03c1fa70817c7a3c7fd661` per Task 16). Two tests:
//!
//!   * `flask_corpus_byte_identical_across_runs` — two `Replay::run` calls
//!     produce byte-identical JSONL output.
//!   * `flask_python_resolver_index_byte_identical_across_runs` — two
//!     `PythonResolver::index` calls produce identical per-symbol
//!     resolutions for a handful of canary names from flask.
//!
//! Both are `#[ignore]` so `cargo test` defaults stay fast; opt in with
//! `cargo test --release -- --ignored determinism_flask`.
//!
//! On corpus mismatch the two runs are dumped to
//! `/tmp/flask-corpus-{a,b}.jsonl` so a reviewer can diff them.

use std::path::{Path, PathBuf};

use provbench_labeler::replay::{FactAtCommit, Replay, ReplayConfig};
use provbench_labeler::resolve::python::PythonResolver;
use provbench_labeler::resolve::SymbolResolver;

const FLASK_T0: &str = "2f0c62f5e6e290843f03c1fa70817c7a3c7fd661";

fn flask_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../work/flask")
}

fn rows_to_jsonl(rows: &[FactAtCommit]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in rows {
        let line = serde_json::to_string(r).expect("serialize FactAtCommit");
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out
}

fn dump_corpora(a: &[u8], b: &[u8]) {
    let pa = Path::new("/tmp/flask-corpus-a.jsonl");
    let pb = Path::new("/tmp/flask-corpus-b.jsonl");
    if let Err(e) = std::fs::write(pa, a) {
        eprintln!("warning: failed to dump corpus A to {}: {e}", pa.display());
    } else {
        eprintln!("dumped corpus A ({} bytes) → {}", a.len(), pa.display());
    }
    if let Err(e) = std::fs::write(pb, b) {
        eprintln!("warning: failed to dump corpus B to {}: {e}", pb.display());
    } else {
        eprintln!("dumped corpus B ({} bytes) → {}", b.len(), pb.display());
    }
}

#[test]
#[ignore = "requires benchmarks/provbench/work/flask checkout; large held-out Python replay"]
fn flask_corpus_byte_identical_across_runs() {
    let flask = flask_path();
    assert!(
        flask.exists(),
        "needs benchmarks/provbench/work/flask checkout pinned at T₀ {FLASK_T0}"
    );
    let cfg = ReplayConfig {
        repo_path: flask,
        t0_sha: FLASK_T0.to_string(),
        skip_symbol_resolution: true,
    };
    let rows1 = Replay::run(&cfg).expect("flask Replay::run #1");
    let rows2 = Replay::run(&cfg).expect("flask Replay::run #2");
    let b1 = rows_to_jsonl(&rows1);
    let b2 = rows_to_jsonl(&rows2);
    if b1 != b2 {
        dump_corpora(&b1, &b2);
        panic!(
            "flask corpus is non-deterministic: run #1 = {} bytes, run #2 = {} bytes",
            b1.len(),
            b2.len()
        );
    }
}

#[test]
#[ignore = "requires benchmarks/provbench/work/flask checkout; held-out Python index pass"]
fn flask_python_resolver_index_byte_identical_across_runs() {
    let flask = flask_path();
    assert!(
        flask.exists(),
        "needs benchmarks/provbench/work/flask checkout pinned at T₀ {FLASK_T0}"
    );

    let mut r1 = PythonResolver::index(&flask).expect("flask index #1");
    let mut r2 = PythonResolver::index(&flask).expect("flask index #2");

    // Canary names — pick stable flask symbols present at T₀.
    // Module paths reflect repo-rooted layout (e.g. `src/flask/app.py`
    // → `src.flask.app`). If flask reorganizes, update this list.
    let names = [
        "src.flask.app.Flask",
        "src.flask.app.Flask.run",
        "src.flask.blueprints.Blueprint",
        "src.flask.helpers.url_for",
        "src.flask.json.tag.JSONTag",
        // Negative canary — must resolve to None on both runs.
        "src.flask.this_symbol_does_not_exist_xyz",
    ];
    for name in names {
        let a = r1.resolve(name).expect("flask resolver #1");
        let b = r2.resolve(name).expect("flask resolver #2");
        assert_eq!(
            a, b,
            "PythonResolver::resolve({name:?}) disagrees across two index() calls on flask"
        );
    }
}
