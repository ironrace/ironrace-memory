use provbench_labeler::label::Label;
use provbench_labeler::replay::{Replay, ReplayConfig};

fn git(repo: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn commit_all(repo: &std::path::Path, message: &str) {
    git(repo, &["add", "."]);
    git(
        repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-m",
            message,
        ],
    );
}

#[test]
#[ignore = "requires pinned rust-analyzer and tree-sitter tooling; run with `cargo test -- --ignored`"]
fn replay_with_rust_analyzer_marks_renamed_function_non_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    git(repo, &["init", "--initial-branch=main"]);
    std::fs::create_dir(repo.join("src")).unwrap();
    std::fs::write(
        repo.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(repo.join("src/lib.rs"), b"pub fn old_name() -> i32 { 1 }\n").unwrap();
    commit_all(repo, "init");
    let t0 = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    std::fs::write(repo.join("src/lib.rs"), b"pub fn new_name() -> i32 { 1 }\n").unwrap();
    commit_all(repo, "rename");

    let rows = Replay::run(&ReplayConfig {
        repo_path: repo.to_path_buf(),
        t0_sha: t0,
        skip_symbol_resolution: false,
    })
    .unwrap();
    assert!(
        rows.iter().any(|row| !matches!(row.label, Label::Valid)),
        "expected at least one non-Valid label for rename, got {rows:?}"
    );
}
