//! Plan A Task 15 — Python labeler determinism on the in-tree fixture.
//!
//! Builds a single-commit git repo from `tests/data/python/repo/` (mirroring
//! the `replay_python_fixture_emits_facts` setup in `tests/replay.rs`) and
//! asserts that two back-to-back runs of:
//!   1. `Replay::run` (corpus rows),
//!   2. `Replay::emit_facts` for every fact_id from run #1 (fact bodies), and
//!   3. `PythonResolver::index` followed by `resolve(name)` for a handful of
//!      fixture symbols,
//!
//! produce byte-identical output.
//!
//! All three tests are default-run (no `#[ignore]`) — the fixture is tiny.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use provbench_labeler::replay::{FactAtCommit, Replay, ReplayConfig};
use provbench_labeler::resolve::python::PythonResolver;
use provbench_labeler::resolve::SymbolResolver;

/// Copy the in-tree Python fixture into `repo_root`, `git init`, commit, and
/// return the resulting HEAD sha. Mirrors `replay_python_fixture_emits_facts`
/// in `tests/replay.rs`.
fn build_python_fixture_repo(repo_root: &Path) -> String {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/python/repo");
    for rel in &["src/example.py", "tests/test_example.py"] {
        let dst = repo_root.join(rel);
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::copy(fixture_root.join(rel), &dst).unwrap();
    }

    let g = |args: &[&str]| {
        let s = Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .status()
            .unwrap();
        assert!(s.success(), "git {args:?} failed");
    };
    g(&["init", "--initial-branch=main"]);
    g(&["add", "-A"]);
    g(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-m",
        "init",
    ]);
    String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_root)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string()
}

/// Serialize a `Vec<FactAtCommit>` into a JSONL byte-vec for byte-equality
/// comparisons (`FactAtCommit` is not `PartialEq`).
fn rows_to_jsonl(rows: &[FactAtCommit]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in rows {
        let line = serde_json::to_string(r).expect("serialize FactAtCommit");
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out
}

#[test]
fn python_fixture_corpus_is_byte_identical_across_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let t0 = build_python_fixture_repo(tmp.path());
    let cfg = ReplayConfig {
        repo_path: tmp.path().to_path_buf(),
        t0_sha: t0,
        // Python branch in Replay does not consult the Rust commit-symbol
        // index; matches the production single-commit fixture pattern.
        skip_symbol_resolution: true,
    };
    let rows1 = Replay::run(&cfg).expect("run #1");
    let rows2 = Replay::run(&cfg).expect("run #2");
    let b1 = rows_to_jsonl(&rows1);
    let b2 = rows_to_jsonl(&rows2);
    assert_eq!(
        b1, b2,
        "python fixture corpus is non-deterministic across two Replay::run calls"
    );
}

#[test]
fn python_fixture_facts_are_byte_identical_across_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let t0 = build_python_fixture_repo(tmp.path());
    let cfg = ReplayConfig {
        repo_path: tmp.path().to_path_buf(),
        t0_sha: t0,
        skip_symbol_resolution: true,
    };
    let rows = Replay::run(&cfg).expect("run for fact_id set");
    let wanted: BTreeSet<String> = rows.iter().map(|r| r.fact_id.clone()).collect();
    assert!(
        !wanted.is_empty(),
        "fixture produced no facts; emit-facts determinism would be vacuous"
    );

    let facts1 = Replay::emit_facts(&cfg, &wanted).expect("emit-facts #1");
    let facts2 = Replay::emit_facts(&cfg, &wanted).expect("emit-facts #2");
    let to_jsonl = |fs: &[provbench_labeler::output::FactBodyRow]| -> Vec<u8> {
        let mut out = Vec::new();
        for f in fs {
            let line = serde_json::to_string(f).expect("serialize FactBodyRow");
            out.extend_from_slice(line.as_bytes());
            out.push(b'\n');
        }
        out
    };
    assert_eq!(
        to_jsonl(&facts1),
        to_jsonl(&facts2),
        "python fixture emit-facts output is non-deterministic"
    );
}

#[test]
fn python_resolver_index_is_byte_identical_across_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let _t0 = build_python_fixture_repo(tmp.path());

    let mut r1 = PythonResolver::index(tmp.path()).expect("index #1");
    let mut r2 = PythonResolver::index(tmp.path()).expect("index #2");

    // Spot-check a handful of names drawn from the fixture:
    //   src/example.py defines CONSTANT_X, Greeter, Greeter.greet,
    //   Greeter.greeting, async_op, _private.
    //   tests/test_example.py exists too but holds no module-level symbols
    //   we'd hit here.
    let names = [
        "src.example.CONSTANT_X",
        "src.example.Greeter",
        "src.example.Greeter.greet",
        "src.example.Greeter.greeting",
        "src.example.async_op",
        "src.example._private",
        // Negative case — must resolve to None on both runs.
        "src.example.does_not_exist",
    ];
    for name in names {
        let a = r1.resolve(name).expect("resolver #1");
        let b = r2.resolve(name).expect("resolver #2");
        assert_eq!(
            a, b,
            "PythonResolver::resolve({name:?}) disagrees across two index() calls"
        );
    }
}
