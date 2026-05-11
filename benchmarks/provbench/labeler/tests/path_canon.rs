//! Integration tests for path canonicalization in fact_id derivation
//! (#1+#7). These tests guarantee that:
//!   1. Replaying two repos that contain byte-identical content but live
//!      at different filesystem paths produces byte-identical `fact_id`s.
//!      (Pure-helper tests for `normalize_path_for_fact_id` live as unit
//!      tests in `src/repo.rs` since the helper is `pub(crate)`.)
//!   2. No emitted `fact_id` ever embeds an absolute filesystem path.

mod common;

use common::{commit_all_with_date, git, rev_parse_head};
use provbench_labeler::replay::{Replay, ReplayConfig};
use std::path::Path;

/// Build a synthetic two-commit repo with deterministic content. Returns
/// the T0 SHA. The same `date_a`/`date_b` values produce the same commit
/// SHAs across calls when used with the same content, which makes the
/// cross-tempdir comparison fully byte-identical.
fn build_synthetic_repo(p: &Path, date_a: &str, date_b: &str) -> String {
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub fn ten() -> i32 { 10 }\npub fn eleven() -> i32 { 11 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", date_a);
    let t0 = rev_parse_head(p);
    // Second commit so replay covers >1 commit.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub fn ten() -> i32 { 10 }\npub fn eleven() -> i32 { 22 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "tweak", date_b);
    t0
}

#[test]
fn fact_ids_byte_identical_across_different_repo_paths() {
    // Build two synthetic repos in two distinct tempdirs with byte-
    // identical content. Their absolute filesystem paths differ (each
    // tempdir is unique), so `Pilot::open` produces different
    // `repo_path` values after `canonicalize`. The fact_id strings,
    // however, must match byte-for-byte: that's exactly what proves
    // fact_ids are repo-relative and never embed the canonicalized
    // root.
    //
    // This test deliberately does NOT manipulate process cwd —
    // `set_current_dir` is process-global and races other tests under
    // cargo's parallel test runner.
    let date_a = "2025-01-01T00:00:00Z";
    let date_b = "2025-01-02T00:00:00Z";

    let tmp_a = tempfile::tempdir().unwrap();
    let tmp_b = tempfile::tempdir().unwrap();
    assert_ne!(
        tmp_a.path().canonicalize().unwrap(),
        tmp_b.path().canonicalize().unwrap(),
        "test setup error: tempdirs must be at distinct absolute paths"
    );

    let t0_a = build_synthetic_repo(tmp_a.path(), date_a, date_b);
    let t0_b = build_synthetic_repo(tmp_b.path(), date_a, date_b);

    let rows_a = Replay::run(&ReplayConfig {
        repo_path: tmp_a.path().to_path_buf(),
        t0_sha: t0_a.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();
    let rows_b = Replay::run(&ReplayConfig {
        repo_path: tmp_b.path().to_path_buf(),
        t0_sha: t0_b.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();

    assert_eq!(
        rows_a.len(),
        rows_b.len(),
        "row counts differ between two byte-identical repos at different paths"
    );
    for (a, b) in rows_a.iter().zip(rows_b.iter()) {
        assert_eq!(
            a.fact_id, b.fact_id,
            "fact_id differs between two byte-identical repos at different paths: {} vs {}",
            a.fact_id, b.fact_id
        );
        assert_eq!(a.label, b.label, "label differs for fact_id {}", a.fact_id);
    }
}

#[test]
fn no_fact_id_contains_absolute_path() {
    // Build a synthetic repo, run replay, and assert no emitted fact_id
    // looks like an absolute path on any platform.
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    let t0 = build_synthetic_repo(p, "2025-01-01T00:00:00Z", "2025-01-02T00:00:00Z");

    let rows = Replay::run(&ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0,
        skip_symbol_resolution: true,
    })
    .unwrap();
    assert!(!rows.is_empty(), "replay returned no rows");

    // Manual scan: cheaper and clearer than pulling in regex as a dev-dep.
    fn looks_windows_absolute(s: &str) -> bool {
        // e.g. "C:\\foo" or "C:/foo"
        let bytes = s.as_bytes();
        // Scan for a drive-letter pattern anywhere; fact_ids are short.
        bytes.windows(3).any(|w| {
            (w[0].is_ascii_uppercase() || w[0].is_ascii_lowercase())
                && w[1] == b':'
                && (w[2] == b'\\' || w[2] == b'/')
        })
    }

    for row in &rows {
        let id = &row.fact_id;
        assert!(
            !id.contains("/Users/"),
            "fact_id leaks /Users/ absolute path: {id}"
        );
        assert!(
            !id.contains("/home/"),
            "fact_id leaks /home/ absolute path: {id}"
        );
        assert!(
            !id.contains("/private/"),
            "fact_id leaks /private/ absolute path: {id}"
        );
        assert!(
            !id.starts_with('/'),
            "fact_id starts with '/' (absolute path): {id}"
        );
        assert!(
            !looks_windows_absolute(id),
            "fact_id looks like a Windows absolute path: {id}"
        );
        // Sanity: every fact_id must mention the repo-relative source path.
        assert!(
            id.contains("src/lib.rs"),
            "fact_id missing expected repo-relative segment 'src/lib.rs': {id}"
        );
    }
}
