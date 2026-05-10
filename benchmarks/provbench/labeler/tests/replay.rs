//! Integration test for the per-commit replay driver.
//!
//! Builds a tiny 2-commit synthetic repo in a temp dir, runs `Replay::run`,
//! and verifies the emitted `FactAtCommit` rows match expectations.
//!
//! NOTE: The second commit changes only the function body (`{ 10 }` → `{ 11
//! }`). The signature span ends at `-> i32` (before the body brace), so the
//! signature hash is identical across both commits.  Both rows should be
//! `Label::Valid`.

use provbench_labeler::replay::{validate_sha_hex, Replay, ReplayConfig};

#[test]
fn validate_sha_hex_rejects_argument_like_and_non_hex_values() {
    assert!(validate_sha_hex("-p HEAD:./evil").is_err());
    assert!(validate_sha_hex("01234567890123456789012345678901234567zz").is_err());
    assert!(validate_sha_hex("0123456789012345678901234567890123456789").is_ok());
}

#[test]
fn replay_over_synthetic_repo_emits_fact_at_commit_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    let g = |args: &[&str]| {
        let s = std::process::Command::new("git")
            .args(args)
            .current_dir(p)
            .status()
            .unwrap();
        assert!(s.success(), "git {args:?} failed");
    };
    g(&["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(p.join("src/lib.rs"), b"pub fn ten() -> i32 { 10 }\n").unwrap();
    g(&["add", "."]);
    g(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-m",
        "init",
    ]);
    let t0 = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(p)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    std::fs::write(p.join("src/lib.rs"), b"pub fn ten() -> i32 { 11 }\n").unwrap();
    g(&["add", "."]);
    g(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-m",
        "tweak",
    ]);
    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    };
    let rows = Replay::run(&cfg).unwrap();
    // 1 fact (function signature `ten`) × 2 commits = 2 rows.
    assert_eq!(rows.len(), 2, "got {rows:?}");
    let labels: Vec<_> = rows.iter().map(|r| r.label.clone()).collect();
    assert!(
        labels
            .iter()
            .any(|l| matches!(l, provbench_labeler::label::Label::Valid)),
        "expected at least one Valid label, got {labels:?}"
    );
    // The second commit changed the function body (`{ 10 }` → `{ 11 }`).
    // The signature span ends at `-> i32`, so the body change does NOT affect
    // the signature hash.  Both rows should be Valid.
    assert!(
        labels
            .iter()
            .all(|l| matches!(l, provbench_labeler::label::Label::Valid)),
        "expected all Valid (body-only change preserves signature hash), got {labels:?}"
    );
}
