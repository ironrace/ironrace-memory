//! Shared synthetic-repo helpers for the labeler integration tests.
//!
//! Lives in `tests/common/mod.rs` (the trailing `/mod.rs` form) so Cargo
//! treats it as a shared module rather than its own test binary, and so
//! each `tests/*.rs` file that consumes these helpers stays a single
//! compilation unit.
//!
//! Cargo compiles each `tests/*.rs` integration test as an independent
//! binary that pulls in the *whole* `common` module; any helper not used
//! by that specific binary would otherwise trip `dead_code` under
//! `-D warnings`. The crate-wide allow below is the standard idiom for
//! `tests/common/mod.rs` files.
#![allow(dead_code)]

use std::path::Path;

/// Run `git <args>` with `repo` as the working directory and assert
/// success. The labeler test suite uses the system `git` binary so
/// these helpers don't depend on `gix` write APIs.
pub fn git(repo: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// `git add . && git commit` with pinned author/committer name, email,
/// and date so two tempdirs that receive byte-identical content produce
/// structurally identical commits (same SHA). User-global hooks run
/// normally; use [`commit_all_with_date_no_verify`] when a fixture
/// must contain bytes a global pre-commit hook would reject (e.g.
/// invalid-UTF-8 README content for hardening regressions).
pub fn commit_all_with_date(repo: &Path, message: &str, date: &str) {
    commit_all_inner(repo, message, date, false);
}

/// Same as [`commit_all_with_date`] but passes `--no-verify` so any
/// user-global pre-commit hook (e.g. a formatter that rejects invalid
/// UTF-8) does not interfere with the synthetic fixture.
pub fn commit_all_with_date_no_verify(repo: &Path, message: &str, date: &str) {
    commit_all_inner(repo, message, date, true);
}

fn commit_all_inner(repo: &Path, message: &str, date: &str, no_verify: bool) {
    git(repo, &["add", "."]);
    let mut args: Vec<&str> = vec!["-c", "user.name=t", "-c", "user.email=t@t", "commit"];
    if no_verify {
        args.push("--no-verify");
    }
    args.extend_from_slice(&["--date", date, "-m", message]);
    let status = std::process::Command::new("git")
        .args(&args)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git commit failed");
}

/// Return the current `HEAD` SHA of `repo` as a trimmed string.
pub fn rev_parse_head(repo: &Path) -> String {
    String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string()
}
