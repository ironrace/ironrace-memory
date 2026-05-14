//! Verifies `emit-facts` produces one well-formed `FactBodyRow` per unique
//! `fact_id` referenced in a corpus.
//!
//! Adaptation notes (vs. the Phase 0c plan's draft):
//! - The plan referenced a `tests/fixtures/tiny-repo` directory that does
//!   not exist in this crate. Other integration tests (`determinism.rs`,
//!   `replay.rs`) build their fixtures on the fly via `tempfile::tempdir`,
//!   so this test follows the same convention: it materializes a tiny
//!   synthetic repo in a tempdir with one `function_item`, one struct
//!   `field`, one `pub fn` (public symbol), one README mention, and one
//!   `#[test]` assertion — covering all five SPEC §3 fact kinds.
//! - The corpus fixture is generated from the real T₀ fact ids (rather
//!   than hard-coded synthetic ones), so the round-trip exercises the
//!   actual extractor → emit-facts → JSONL writer path.

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Build a synthetic single-commit repo containing one example of every
/// SPEC §3 fact kind. Returns `(tempdir, repo_path, t0_sha)`.
fn make_tiny_repo() -> (TempDir, std::path::PathBuf, String) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().to_path_buf();
    common::git(&repo, &["init", "--initial-branch=main"]);
    std::fs::write(
        repo.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir(repo.join("src")).unwrap();
    // `pub fn add` → FunctionSignature + PublicSymbol.
    // `pub struct Point { pub x: i32 }` → Field + PublicSymbol.
    // `#[test] fn it_adds` containing `assert_eq!(add(...), ...)` → TestAssertion.
    std::fs::write(
        repo.join("src/lib.rs"),
        b"pub fn add(a: i32, b: i32) -> i32 { a + b }\n\
          pub struct Point { pub x: i32 }\n\
          #[cfg(test)]\n\
          mod t {\n    \
              use super::*;\n    \
              #[test]\n    \
              fn it_adds() { assert_eq!(add(1, 2), 3); }\n\
          }\n",
    )
    .unwrap();
    // README mentioning `add` → DocClaim.
    std::fs::write(
        repo.join("README.md"),
        b"# x\n\nThe `add` function returns the sum.\n",
    )
    .unwrap();
    common::commit_all_with_date(&repo, "init", "2026-05-13T00:00:00Z");
    let t0 = common::rev_parse_head(&repo);
    (tmp, repo, t0)
}

/// Run the real labeler over the synthetic repo and harvest the set of
/// fact_ids it emits at T₀. We do this by invoking the binary's
/// production `Run` command and re-parsing the JSONL.
fn corpus_fact_ids(bin: &str, repo: &Path, t0: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("corpus.jsonl");
    let status = Command::new(bin)
        .args([
            "run",
            "--repo",
            repo.to_str().unwrap(),
            "--t0",
            t0,
            "--out",
            out.to_str().unwrap(),
            "--skip-symbol-resolution",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "labeler run failed");
    let body = std::fs::read_to_string(&out).unwrap();
    let mut ids = BTreeSet::new();
    for line in body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        ids.insert(v["fact_id"].as_str().unwrap().to_string());
    }
    ids.into_iter().collect()
}

#[test]
fn emit_facts_writes_one_row_per_unique_fact_id() {
    let bin = env!("CARGO_BIN_EXE_provbench-labeler");
    let (_tmp, repo, t0) = make_tiny_repo();
    let fact_ids = corpus_fact_ids(bin, &repo, &t0);
    assert!(
        fact_ids.len() >= 5,
        "synthetic repo should expose all five fact kinds; got {:?}",
        fact_ids
    );

    // Synthesize a corpus that references every fact_id at least once
    // (some twice across two fake commit shas to exercise dedup).
    let corpus_dir = tempfile::tempdir().unwrap();
    let corpus_path = corpus_dir.path().join("corpus.jsonl");
    let mut corpus_lines = String::new();
    for (i, id) in fact_ids.iter().enumerate() {
        let sha_a = "a".repeat(40);
        let sha_b = "b".repeat(40);
        corpus_lines.push_str(&format!(
            "{{\"fact_id\":{id},\"commit_sha\":\"{sha_a}\",\"label\":\"Valid\",\"labeler_git_sha\":\"X\"}}\n",
            id = serde_json::to_string(id).unwrap(),
            sha_a = sha_a,
        ));
        if i % 2 == 0 {
            corpus_lines.push_str(&format!(
                "{{\"fact_id\":{id},\"commit_sha\":\"{sha_b}\",\"label\":\"Valid\",\"labeler_git_sha\":\"X\"}}\n",
                id = serde_json::to_string(id).unwrap(),
                sha_b = sha_b,
            ));
        }
    }
    std::fs::write(&corpus_path, corpus_lines).unwrap();

    let out = corpus_dir.path().join("facts.jsonl");
    let status = Command::new(bin)
        .args([
            "emit-facts",
            "--corpus",
            corpus_path.to_str().unwrap(),
            "--repo",
            repo.to_str().unwrap(),
            "--t0",
            &t0,
            "--out",
            out.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "emit-facts must exit 0");

    let body = std::fs::read_to_string(&out).unwrap();
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        fact_ids.len(),
        "expected one fact body per unique fact_id"
    );

    // Schema check: every row has the required fields with sensible types.
    let mut seen_kinds: BTreeSet<String> = BTreeSet::new();
    let mut prev_fact_id: Option<String> = None;
    for line in &lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        for required in [
            "fact_id",
            "kind",
            "body",
            "source_path",
            "line_span",
            "symbol_path",
            "content_hash_at_observation",
            "labeler_git_sha",
        ] {
            assert!(
                v.get(required).is_some(),
                "field `{required}` missing from row: {line}"
            );
        }
        let fact_id = v["fact_id"].as_str().unwrap().to_string();
        if let Some(prev) = &prev_fact_id {
            assert!(
                prev.as_str() < fact_id.as_str(),
                "rows must be sorted ascending by fact_id (got {prev:?} before {fact_id:?})"
            );
        }
        prev_fact_id = Some(fact_id);

        let line_span = v["line_span"].as_array().unwrap();
        assert_eq!(line_span.len(), 2, "line_span must be [start, end]");
        for n in line_span {
            assert!(n.as_u64().is_some(), "line_span entries must be u32");
        }
        let hash = v["content_hash_at_observation"].as_str().unwrap();
        assert_eq!(hash.len(), 64, "content_hash must be 64 hex chars");
        assert!(
            hash.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "content_hash must be lowercase hex"
        );
        let body_text = v["body"].as_str().unwrap();
        assert!(
            !body_text.is_empty(),
            "body must be a non-empty SPEC §3 claim string"
        );
        seen_kinds.insert(v["kind"].as_str().unwrap().to_string());
    }
    // The synthetic repo exposes all five SPEC §3 fact kinds.
    let expected_kinds: BTreeSet<String> = [
        "FunctionSignature",
        "Field",
        "PublicSymbol",
        "DocClaim",
        "TestAssertion",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert!(
        seen_kinds.is_superset(&expected_kinds),
        "missing kinds: got {seen_kinds:?}, expected at least {expected_kinds:?}"
    );

    // Determinism: re-run produces byte-identical output.
    let out2 = corpus_dir.path().join("facts2.jsonl");
    let status2 = Command::new(bin)
        .args([
            "emit-facts",
            "--corpus",
            corpus_path.to_str().unwrap(),
            "--repo",
            repo.to_str().unwrap(),
            "--t0",
            &t0,
            "--out",
            out2.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status2.success(), "emit-facts re-run must exit 0");
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(&out2).unwrap(),
        "emit-facts must be byte-deterministic"
    );
}
