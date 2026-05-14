//! Verifies `emit-diffs` produces one `<commit_sha>.json` per distinct
//! `commit_sha` referenced in a corpus, with full-file-context unified
//! diffs (SPEC §6.1).
//!
//! Adaptation notes (vs. the Phase 0c plan's draft):
//! - The plan referenced `FIXTURE_T0` / `FIXTURE_NEXT_COMMIT` placeholders.
//!   No checked-in fixture exists; this test follows the same convention
//!   as `emit_facts_roundtrip.rs` and `determinism.rs` and builds a small
//!   synthetic repo in a tempdir via `git init` + arg-vector commits.
//! - The synthetic repo has three first-parent commits:
//!   T₀ (initial commit with `src/lib.rs` containing one `pub fn`),
//!   T₀^1 (modify `src/lib.rs` — change function body), and
//!   T₀^2 (add `src/util.rs`). Both non-T₀ commits exercise the
//!   `Included` path; T₀ exercises the `excluded:"t0"` path.
//! - Determinism is checked by re-running the command into a second
//!   output directory and comparing the resulting JSON file bytes.

mod common;

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Build a synthetic 3-commit repo. Returns
/// `(tempdir, repo_path, t0_sha, t1_sha, t2_sha)`.
fn make_three_commit_repo() -> (TempDir, std::path::PathBuf, String, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().to_path_buf();
    common::git(&repo, &["init", "--initial-branch=main"]);

    // T₀: initial commit.
    std::fs::create_dir(repo.join("src")).unwrap();
    std::fs::write(
        repo.join("src/lib.rs"),
        b"pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();
    common::commit_all_with_date(&repo, "t0", "2026-05-13T00:00:00Z");
    let t0 = common::rev_parse_head(&repo);

    // T₀^1: modify the existing file.
    std::fs::write(
        repo.join("src/lib.rs"),
        b"pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )
    .unwrap();
    common::commit_all_with_date(&repo, "t1", "2026-05-13T00:00:01Z");
    let t1 = common::rev_parse_head(&repo);

    // T₀^2: add a second file.
    std::fs::write(
        repo.join("src/util.rs"),
        b"pub fn double(n: i32) -> i32 { n * 2 }\n",
    )
    .unwrap();
    common::commit_all_with_date(&repo, "t2", "2026-05-13T00:00:02Z");
    let t2 = common::rev_parse_head(&repo);

    (tmp, repo, t0, t1, t2)
}

/// Write a synthetic corpus JSONL that references the three commits.
/// The labeler `Run` output schema is `{fact_id, commit_sha, label,
/// labeler_git_sha}` per line; we only need `commit_sha` to be readable,
/// but the JSON is well-formed and parseable as `OutputRow`.
fn write_synthetic_corpus(corpus: &Path, commits: &[&str]) {
    let mut body = String::new();
    for sha in commits {
        body.push_str(&format!(
            "{{\"fact_id\":\"fixt-{sha}\",\"commit_sha\":\"{sha}\",\"label\":\"Valid\",\"labeler_git_sha\":\"X\"}}\n",
            sha = sha,
        ));
    }
    std::fs::write(corpus, body).unwrap();
}

#[test]
fn emit_diffs_writes_one_artifact_per_distinct_commit() {
    let bin = env!("CARGO_BIN_EXE_provbench-labeler");
    let (_tmp, repo, t0, t1, t2) = make_three_commit_repo();

    let work = tempfile::tempdir().unwrap();
    let corpus = work.path().join("corpus.jsonl");
    write_synthetic_corpus(&corpus, &[&t0, &t1, &t2]);

    let out_dir = work.path().join("diffs");
    let status = Command::new(bin)
        .args([
            "emit-diffs",
            "--corpus",
            corpus.to_str().unwrap(),
            "--repo",
            repo.to_str().unwrap(),
            "--t0",
            &t0,
            "--out-dir",
            out_dir.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "emit-diffs must exit 0");

    // ── T₀: excluded:"t0" ──────────────────────────────────────────────
    let t0_path = out_dir.join(format!("{t0}.json"));
    let t0_v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&t0_path).unwrap()).unwrap();
    assert_eq!(
        t0_v["commit_sha"].as_str(),
        Some(t0.as_str()),
        "T₀ artifact must carry its commit_sha"
    );
    assert_eq!(
        t0_v["excluded"].as_str(),
        Some("t0"),
        "T₀ artifact must be excluded with reason `t0`"
    );
    assert!(
        t0_v.get("unified_diff").is_none(),
        "T₀ artifact must not carry a unified_diff"
    );

    // ── T₀^1: included, modifies src/lib.rs ────────────────────────────
    let t1_path = out_dir.join(format!("{t1}.json"));
    let t1_v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&t1_path).unwrap()).unwrap();
    assert_eq!(t1_v["commit_sha"].as_str(), Some(t1.as_str()));
    assert_eq!(
        t1_v["parent_sha"].as_str(),
        Some(t0.as_str()),
        "T₀^1 parent must be T₀"
    );
    let t1_diff = t1_v["unified_diff"].as_str().expect("unified_diff present");
    assert!(!t1_diff.is_empty(), "T₀^1 unified_diff must be non-empty");
    assert!(
        t1_diff.contains("diff --git") || t1_diff.contains("--- a/"),
        "T₀^1 unified_diff must look like unified diff output; got: {}",
        &t1_diff[..t1_diff.len().min(200)]
    );
    assert!(
        t1_diff.contains("+++ b/"),
        "T₀^1 unified_diff must contain `+++ b/` marker"
    );
    assert!(
        t1_diff.contains("src/lib.rs"),
        "T₀^1 unified_diff must touch src/lib.rs"
    );

    // ── T₀^2: included, adds src/util.rs ───────────────────────────────
    let t2_path = out_dir.join(format!("{t2}.json"));
    let t2_v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&t2_path).unwrap()).unwrap();
    assert_eq!(t2_v["commit_sha"].as_str(), Some(t2.as_str()));
    assert_eq!(
        t2_v["parent_sha"].as_str(),
        Some(t1.as_str()),
        "T₀^2 parent must be T₀^1"
    );
    let t2_diff = t2_v["unified_diff"].as_str().expect("unified_diff present");
    assert!(!t2_diff.is_empty(), "T₀^2 unified_diff must be non-empty");
    assert!(
        t2_diff.contains("--- a/") || t2_diff.contains("diff --git"),
        "T₀^2 unified_diff must look like unified diff output"
    );
    assert!(
        t2_diff.contains("src/util.rs"),
        "T₀^2 unified_diff must touch src/util.rs"
    );

    // ── Determinism: re-run produces byte-identical artifacts ─────────
    let out_dir2 = work.path().join("diffs2");
    let status2 = Command::new(bin)
        .args([
            "emit-diffs",
            "--corpus",
            corpus.to_str().unwrap(),
            "--repo",
            repo.to_str().unwrap(),
            "--t0",
            &t0,
            "--out-dir",
            out_dir2.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status2.success(), "emit-diffs re-run must exit 0");
    for sha in [&t0, &t1, &t2] {
        let p1 = out_dir.join(format!("{sha}.json"));
        let p2 = out_dir2.join(format!("{sha}.json"));
        assert_eq!(
            std::fs::read(&p1).unwrap(),
            std::fs::read(&p2).unwrap(),
            "emit-diffs must be byte-deterministic for {sha}",
        );
    }
}
