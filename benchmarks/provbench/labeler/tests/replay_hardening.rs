//! Targeted integration regressions for the hardening pass-2 surface
//! (Tasks 1, 4, and a non-function fact-type label transition).
//!
//! Each test exercises the FULL replay loop via the public `Replay::run`
//! API so that any regression in helper internals (path canonicalization,
//! UTF-8 error context, field-fact label classification) is caught at
//! integration level, not just at the unit level.

use provbench_labeler::label::Label;
use provbench_labeler::output::{write_jsonl, OutputRow};
use provbench_labeler::replay::{FactAtCommit, Replay, ReplayConfig};
use std::path::Path;

// ── shared synthetic-repo helpers ────────────────────────────────────────────

fn git(repo: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn commit_all_with_date(repo: &Path, message: &str, date: &str) {
    git(repo, &["add", "."]);
    // `--no-verify` disables any user-global git hook (e.g. a pre-commit
    // formatter) that might choke on synthetic invalid-UTF-8 README
    // bytes used by the hardening regression tests.
    let status = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "--no-verify",
            "--date",
            date,
            "-m",
            message,
        ])
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git commit failed");
}

fn rev_parse_head(repo: &Path) -> String {
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

fn to_output_rows(rows: Vec<FactAtCommit>) -> Vec<OutputRow> {
    rows.into_iter()
        .map(|r| OutputRow {
            fact_id: r.fact_id,
            commit_sha: r.commit_sha,
            label: r.label,
        })
        .collect()
}

// ── Test 1: full JSONL byte-identity across two repo paths ───────────────────

/// Build a synthetic repo with two commits, mixed fact types (function
/// signature + DocClaim via README inline-code mention). The same content
/// at two different absolute filesystem paths must produce byte-identical
/// JSONL output — proving no part of the replay loop (error context,
/// debug strings, file paths) leaks an absolute path into the emitted
/// rows or labeler-stamped JSON.
fn build_mixed_repo(repo: &Path, date_a: &str, date_b: &str) -> String {
    git(repo, &["init", "--initial-branch=main"]);
    std::fs::create_dir(repo.join("src")).unwrap();
    std::fs::write(
        repo.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("src/lib.rs"),
        b"pub fn search() -> i32 { 1 }\npub fn lookup() -> i32 { 2 }\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("README.md"),
        b"Use `search` and `lookup` to find data.\n",
    )
    .unwrap();
    commit_all_with_date(repo, "init", date_a);
    let t0 = rev_parse_head(repo);
    // Second commit: change a body so the function signature stays identical
    // but the function body differs — exercises the >1-commit code path.
    std::fs::write(
        repo.join("src/lib.rs"),
        b"pub fn search() -> i32 { 11 }\npub fn lookup() -> i32 { 2 }\n",
    )
    .unwrap();
    commit_all_with_date(repo, "tweak", date_b);
    t0
}

#[test]
fn full_jsonl_output_byte_identical_across_different_repo_paths() {
    let date_a = "2025-01-01T00:00:00Z";
    let date_b = "2025-01-02T00:00:00Z";

    let tmp_a = tempfile::tempdir().unwrap();
    let tmp_b = tempfile::tempdir().unwrap();
    assert_ne!(
        tmp_a.path().canonicalize().unwrap(),
        tmp_b.path().canonicalize().unwrap(),
        "test setup error: tempdirs must be at distinct absolute paths"
    );

    let t0_a = build_mixed_repo(tmp_a.path(), date_a, date_b);
    let t0_b = build_mixed_repo(tmp_b.path(), date_a, date_b);

    let rows_a = to_output_rows(
        Replay::run(&ReplayConfig {
            repo_path: tmp_a.path().to_path_buf(),
            t0_sha: t0_a.clone(),
            skip_symbol_resolution: true,
        })
        .unwrap(),
    );
    let rows_b = to_output_rows(
        Replay::run(&ReplayConfig {
            repo_path: tmp_b.path().to_path_buf(),
            t0_sha: t0_b.clone(),
            skip_symbol_resolution: true,
        })
        .unwrap(),
    );

    // Sanity: the synthetic content must produce a non-trivial mix that
    // includes a DocClaim (the README mentions `search` and `lookup`).
    assert!(
        rows_a.iter().any(|r| r.fact_id.starts_with("DocClaim::")),
        "test setup did not emit any DocClaim rows: {rows_a:?}"
    );
    assert!(
        rows_a
            .iter()
            .any(|r| r.fact_id.starts_with("FunctionSignature::")),
        "test setup did not emit any FunctionSignature rows: {rows_a:?}"
    );

    // Pinned dates yield identical commit SHAs across the two tempdirs;
    // pinned content yields identical fact_ids. Therefore the FULL JSONL
    // output (sorted, labeler-SHA-stamped) must be byte-identical.
    let out_a = tmp_a.path().join("out.jsonl");
    let out_b = tmp_b.path().join("out.jsonl");
    write_jsonl(&out_a, &rows_a, "ffffffffffffffffffffffffffffffffffffffff").unwrap();
    write_jsonl(&out_b, &rows_b, "ffffffffffffffffffffffffffffffffffffffff").unwrap();

    let bytes_a = std::fs::read(&out_a).unwrap();
    let bytes_b = std::fs::read(&out_b).unwrap();
    assert_eq!(
        bytes_a, bytes_b,
        "full JSONL output diverged across two byte-identical repos at different paths;\
         this means some part of the replay loop is leaking an absolute path."
    );

    // Defence-in-depth: scan the full JSONL bytes for absolute-path
    // markers. Catches regressions where a path leak would only matter
    // on Linux/Windows but is invisible on macOS, or vice versa.
    let text = String::from_utf8(bytes_a).unwrap();
    for needle in &["/Users/", "/home/", "/private/", "/tmp/", "/var/folders/"] {
        assert!(
            !text.contains(needle),
            "JSONL output leaks absolute-path marker {needle:?}: {text}"
        );
    }
}

// ── Test 2: invalid-UTF-8 README → contextual error from full replay ─────────

/// At T0 the README contains the invalid UTF-8 sequence `[0xC3, 0x28]`
/// (a known-bad 2-byte sequence: `0xC3` starts a 2-byte codepoint but
/// `0x28` is not a valid continuation byte). Doc-claim extraction runs
/// at T0 inside `Replay::run`, so the from_utf8 error must propagate
/// out of replay with a context message that mentions BOTH the README
/// path AND the offending commit SHA.
///
/// NOTE: `replay.rs` only invokes `doc_claim::extract` for the T0 blob
/// (post-commit doc blobs are handled span-byte-only and never go
/// through `from_utf8`). Therefore the invalid bytes must live at T0
/// to exercise the full error chain — replay → extract → from_utf8 →
/// `with_context(README path @ commit SHA)`.
#[test]
fn invalid_utf8_readme_at_t0_surfaces_path_and_sha_in_error() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(p.join("src/lib.rs"), b"pub fn search() -> i32 { 1 }\n").unwrap();
    // README starts with a recognizable ASCII prefix so the error message
    // contains the literal `README.md` filename, then the invalid bytes.
    let bad_readme: &[u8] = b"prefix \xC3\x28 suffix\n";
    std::fs::write(p.join("README.md"), bad_readme).unwrap();
    commit_all_with_date(p, "init-with-bad-readme", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // Add a second valid commit so replay walks more than one — confirms
    // the error is raised during T0 extraction, not at a later point.
    std::fs::write(p.join("src/lib.rs"), b"pub fn search() -> i32 { 2 }\n").unwrap();
    commit_all_with_date(p, "tweak", "2025-01-02T00:00:00Z");

    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    };
    let err = Replay::run(&cfg).expect_err("replay must fail on invalid UTF-8 README at T0");

    // `format!("{err:#}")` walks the full anyhow error chain so we see
    // every `with_context` layer, including the `parse README at <path> @ <sha>`
    // wrapper added by Task 4 in replay.rs.
    let msg = format!("{err:#}");
    assert!(
        msg.contains("README.md"),
        "error must include README path, got: {msg}"
    );
    assert!(
        msg.contains(&t0),
        "error must include offending commit SHA {t0}, got: {msg}"
    );
    assert!(
        msg.to_lowercase().contains("utf"),
        "error must mention UTF-8, got: {msg}"
    );
}

// ── Test 3: field type-change transition emits StaleSourceChanged ────────────

/// Field-fact extractor and the structural-change rule already exist;
/// this test guarantees the replay path correctly emits `StaleSourceChanged`
/// for a non-function fact type when the field's type changes between
/// T0 and the next commit (field name unchanged → qualified_path unchanged
/// → post-commit lookup succeeds → content hash differs → first-match-wins
/// rule selects `StaleSourceChanged`).
///
/// Renaming the field would change the qualified_path, which at unit-mode
/// (`skip_symbol_resolution=true`) yields `StaleSourceDeleted` rather than
/// the structural-change label, so we drive the transition with a TYPE
/// change instead — same semantic intent (a non-function fact whose
/// underlying source changed), correct closed-enum variant.
#[test]
fn field_type_change_transitions_to_stale_source_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub struct Config { pub limit: usize }\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // Same field name (`limit`) → qualified_path stays `Config::limit`,
    // so the post-commit lookup succeeds. Type changes from `usize`
    // to `u64` → content hash of the field span differs → label rule
    // §5 emits `StaleSourceChanged`.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub struct Config { pub limit: u64 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "field-type-change", "2025-01-02T00:00:00Z");

    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    };
    let rows = Replay::run(&cfg).unwrap();

    let field_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.fact_id.starts_with("Field::Config::limit::"))
        .collect();
    assert_eq!(
        field_rows.len(),
        2,
        "expected one Field row per commit (T0 + tweak), got {field_rows:?}"
    );

    // T0 itself: the field is byte-identical to the observed fact, so the
    // label is Valid.
    let t0_row = field_rows
        .iter()
        .find(|r| r.commit_sha == t0)
        .expect("T0 row missing");
    assert!(
        matches!(t0_row.label, Label::Valid),
        "T0 field row must be Valid, got {:?}",
        t0_row.label
    );

    // The other commit changed the field's type → StaleSourceChanged.
    let post_row = field_rows
        .iter()
        .find(|r| r.commit_sha != t0)
        .expect("post-T0 row missing");
    assert!(
        matches!(post_row.label, Label::StaleSourceChanged),
        "field type-change row must be StaleSourceChanged, got {:?}",
        post_row.label
    );
}
