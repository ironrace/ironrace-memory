//! Targeted integration regressions for the hardening pass-2 surface
//! (Tasks 1, 4, and a non-function fact-type label transition).
//!
//! Each test exercises the FULL replay loop via the public `Replay::run`
//! API so that any regression in helper internals (path canonicalization,
//! UTF-8 error context, field-fact label classification) is caught at
//! integration level, not just at the unit level.

mod common;

use common::{commit_all_with_date_no_verify as commit_all_with_date, git, rev_parse_head};
use provbench_labeler::diff::rename_candidate;
use provbench_labeler::label::Label;
use provbench_labeler::output::{write_jsonl, OutputRow};
use provbench_labeler::replay::{FactAtCommit, Replay, ReplayConfig};
use provbench_labeler::resolve::{ResolvedLocation, SymbolResolver};
use std::path::Path;

// ── shared synthetic-repo helpers ────────────────────────────────────────────
//
// Hardening fixtures here include invalid-UTF-8 bytes that some
// user-global pre-commit hooks reject; we therefore use
// `commit_all_with_date_no_verify` (re-aliased as `commit_all_with_date`
// at this crate scope to keep call sites unchanged).

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

// ── Pass-3 hardening regressions ─────────────────────────────────────────────
//
// RED / GREEN status against HEAD a6b7e5c:
//   HP3-1           RED  (ignore-gated) — Task 2
//   HP3-2 (renamed) GREEN               — preservation: skip_symbol_resolution mode
//   HP3-2 (new)     RED  (ignore-gated) — Task 3 cluster B: per-commit symbol resolution
//   HP3-5           RED  (ignore-gated) — Task 5
//   HP3-4           GREEN               — rename true-positive preservation
//
// Each test is self-contained: synthetic repo, pinned dates, no
// rust-analyzer (HP3-2-new uses a stub resolver), no network, no
// wall-clock dependency.
//
// Anti-tuning note: these fixtures are NOT derived from the 200-row
// diagnostic sample used in the §9.1 gate run.

// ── HP3-1: visibility narrowing → StaleSourceChanged ────────────────────────

/// T0: `pub struct Config { … }`.  Post commit: `pub(crate) struct Config { … }`.
///
/// The symbol still exists in the same file but its visibility was narrowed
/// from bare `pub` to `pub(crate)`.  The labeler should recognise this as a
/// structural change to the public API (`StaleSourceChanged`).
///
/// **Why it fails on HEAD a6b7e5c:**
/// `symbol_existence::extract` only extracts bare-`pub` items, so the post-
/// commit AST yields no `PublicSymbol::Config` entry.  `matching_post_fact`
/// therefore returns `None`, `symbol_resolves = false`, and the rule engine
/// falls through to `StaleSourceDeleted` — even though the struct is still
/// present.  The correct label is `StaleSourceChanged` (visibility-narrowing
/// cluster).
#[test]
fn hp3_1_visibility_narrowing_emits_stale_source_changed() {
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
        b"pub struct Config { pub field: u32 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // Narrow `pub` → `pub(crate)` while keeping the struct name and body
    // identical so the only change is the visibility qualifier.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub(crate) struct Config { pub field: u32 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "narrow-visibility", "2025-01-02T00:00:00Z");

    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    };
    let rows = Replay::run(&cfg).unwrap();

    let sym_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.fact_id.starts_with("PublicSymbol::Config::"))
        .collect();

    let post_row = sym_rows
        .iter()
        .find(|r| r.commit_sha != t0)
        .expect("post-T0 PublicSymbol::Config row missing");

    assert!(
        matches!(post_row.label, Label::StaleSourceChanged),
        "visibility narrowing must emit StaleSourceChanged, got {:?}",
        post_row.label
    );
}

// ── HP3-2 (preservation): skip_symbol_resolution mode is unaffected by Task 3 ─

/// Three-commit history:
///   T0  → `fn replace_with_captures(s: &str) -> String { … }`
///   C1  → function deleted entirely (only `something_else()` remains)
///   C2  → `fn replace_with_caps(s: &str) -> String { … }` added at C2
///
/// In unit-test mode (`skip_symbol_resolution = true`) rename detection is
/// disabled, so both C1 and C2 currently emit `StaleSourceDeleted` — correct.
/// This test is GREEN on HEAD a6b7e5c and serves as a preservation contract:
/// per-commit blob isolation must remain correct even when a similarly-named
/// symbol appears at a later commit, and the `skip_symbol_resolution=true`
/// code path must remain unaffected by Task 3's per-commit-tree fix.
///
/// See `hp3_2_per_commit_symbol_resolution_red` for the companion RED test
/// that exercises the symbol-resolution code path via a stub resolver.
/// See `diff.rs` as `hp3_3b_replacement_deletion_no_false_rename_candidate`
/// for the diff-level contract that the `similar` heuristic must NOT produce
/// a rename candidate between `replace_with_captures` and `replace_with_caps`.
#[test]
fn hp3_2_skip_symbol_resolution_per_commit_preservation() {
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
        b"pub fn replace_with_captures(s: &str) -> String { s.to_string() }\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // C1: delete replace_with_captures entirely.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub fn something_else() -> i32 { 0 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "delete", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    // C2: add replace_with_caps — a new symbol with a similar name.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub fn something_else() -> i32 { 0 }\n\
          pub fn replace_with_caps(s: &str) -> String { s.to_string() }\n",
    )
    .unwrap();
    commit_all_with_date(p, "add-similar", "2025-01-03T00:00:00Z");
    let c2 = rev_parse_head(p);

    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    };
    let rows = Replay::run(&cfg).unwrap();

    let fn_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.fact_id.contains("replace_with_captures"))
        .collect();

    // Both post-T0 commits must classify as StaleSourceDeleted because
    // replace_with_captures was deleted and replace_with_caps is a new symbol.
    let c1_row = fn_rows
        .iter()
        .find(|r| r.commit_sha == c1)
        .expect("C1 row missing");
    let c2_row = fn_rows
        .iter()
        .find(|r| r.commit_sha == c2)
        .expect("C2 row missing");

    assert!(
        matches!(c1_row.label, Label::StaleSourceDeleted),
        "C1 (deleted commit) must be StaleSourceDeleted, got {:?}",
        c1_row.label
    );
    assert!(
        matches!(c2_row.label, Label::StaleSourceDeleted),
        "C2 (similar-sibling commit) must be StaleSourceDeleted, got {:?} — \
         rename heuristic must NOT trigger when the original was deleted and a \
         differently-named symbol was added later",
        c2_row.label
    );
}

// ── HP3-2 (RED): per-commit symbol resolution must use commit-tree, not HEAD ─

/// Three-commit history that exposes the HEAD-keyed resolver bug (Cluster B):
///   T0  → `fn obscure_helper() -> u32 { 0 }`
///   C1  → `fn obscure_helper` deleted; only `unrelated_fn()` remains
///   C2  → NEW `fn obscure_helper() -> u32 { 42 }` added (different body)
///
/// **Failure mode captured (HEAD a6b7e5c):**
/// At C1, `matching_post_fact` finds no `obscure_helper` in the C1 AST →
/// falls into the `None` branch → calls `resolver.resolve("obscure_helper")`.
/// The current code queries the working-tree (HEAD = C2) resolver, which
/// finds the NEW `fn obscure_helper` at C2 → returns `Some(location)` →
/// `symbol_resolves = true`, `post_span_hash = None` → `classify` returns
/// `NeedsRevalidation`.  The correct label at C1 is `StaleSourceDeleted`
/// (the symbol was deleted at that commit; the HEAD re-addition is irrelevant
/// to the per-commit verdict).
///
/// **Why `#[ignore]`:** Task 3's fix will route per-commit resolution through
/// the commit-tree blob index rather than the HEAD-keyed RA instance.  The
/// stub resolver here mirrors the bug: it always reports `obscure_helper`
/// resolves (simulating a HEAD-keyed query that finds C2's copy).  After Task
/// 3, `Replay::run_with_resolver` will no longer use the stub for per-commit
/// classification — it will build a local blob index from each commit's tree —
/// so the stub returning `Some` will not affect the C1 verdict.
///
/// **No rust-analyzer required:** the stub resolver is a simple
/// `HashMap`-backed struct; no RA binary is spawned.
#[test]
fn hp3_2_per_commit_symbol_resolution_red() {
    // ── Stub resolver that always says "symbol resolves" ──────────────────
    // This simulates the HEAD-keyed bug: every `resolve(name)` call returns
    // `Some(location)` regardless of which commit is being classified.
    struct AlwaysResolvesStub;
    impl SymbolResolver for AlwaysResolvesStub {
        fn resolve(&mut self, _qualified_name: &str) -> anyhow::Result<Option<ResolvedLocation>> {
            Ok(Some(ResolvedLocation {
                file: std::path::PathBuf::from("src/lib.rs"),
                line: 1,
            }))
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    // T0: obscure_helper exists.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub fn obscure_helper() -> u32 { 0 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // C1: delete obscure_helper; only unrelated_fn remains.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub fn unrelated_fn() -> u32 { 1 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "delete-obscure-helper", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    // C2: add a NEW fn obscure_helper with a different body.  A HEAD-keyed
    // resolver will see this and report "symbol resolves" for any commit,
    // including C1 where it was actually absent.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub fn unrelated_fn() -> u32 { 1 }\n\
          pub fn obscure_helper() -> u32 { 42 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "re-add-obscure-helper", "2025-01-03T00:00:00Z");

    // Run with the stub resolver.  `skip_symbol_resolution` must be false so
    // that `classify_against_commit` actually consults the resolver when
    // `matching_post_fact` returns None at C1.
    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: false,
    };
    let rows = Replay::run_with_resolver(&cfg, Some(Box::new(AlwaysResolvesStub))).unwrap();

    let fn_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.fact_id.contains("obscure_helper"))
        .collect();

    let c1_row = fn_rows
        .iter()
        .find(|r| r.commit_sha == c1)
        .expect("C1 row for obscure_helper missing");

    // The stub always says "symbol resolves", so the current HEAD-keyed code
    // returns NeedsRevalidation at C1 instead of StaleSourceDeleted.
    // Task 3's fix must ignore the stub (or bypass it) and use per-commit
    // blob presence, correctly returning StaleSourceDeleted.
    assert!(
        matches!(c1_row.label, Label::StaleSourceDeleted),
        "C1 (obscure_helper deleted) must be StaleSourceDeleted even when the \
         HEAD-keyed stub reports the symbol resolves; got {:?} — Task 3 must \
         use per-commit-tree resolution, not the working-tree resolver",
        c1_row.label
    );
}

/// Regression for the commit-index path source itself:
///   T0  → `relocated_helper` exists in `src/lib.rs`
///   C1  → `relocated_helper` moves to a newly-added `src/relocated.rs`
///
/// A commit-tree-local index must enumerate `.rs` paths from the commit being
/// classified, not just paths that existed at T0. Otherwise newly-added files
/// are invisible and this move is misclassified as `StaleSourceDeleted`.
#[test]
fn hp3_2b_commit_index_sees_new_rust_files_added_after_t0() {
    struct NullResolver;
    impl SymbolResolver for NullResolver {
        fn resolve(&mut self, _qualified_name: &str) -> anyhow::Result<Option<ResolvedLocation>> {
            Ok(None)
        }
    }

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
        b"pub fn relocated_helper() -> u32 { 7 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    std::fs::write(
        p.join("src/lib.rs"),
        b"pub fn unrelated_fn() -> u32 { 1 }\n",
    )
    .unwrap();
    std::fs::write(
        p.join("src/relocated.rs"),
        b"pub fn relocated_helper() -> u32 { 7 }\n",
    )
    .unwrap();
    commit_all_with_date(
        p,
        "move-relocated-helper-to-new-file",
        "2025-01-02T00:00:00Z",
    );
    let c1 = rev_parse_head(p);

    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0,
        skip_symbol_resolution: false,
    };
    let rows = Replay::run_with_resolver(&cfg, Some(Box::new(NullResolver))).unwrap();

    let moved_row = rows
        .iter()
        .find(|r| r.fact_id.contains("relocated_helper") && r.commit_sha == c1)
        .expect("C1 row for relocated_helper missing");

    assert!(
        matches!(moved_row.label, Label::NeedsRevalidation),
        "same-qualified symbol moved to a newly-added file must route to \
         NeedsRevalidation, got {:?}",
        moved_row.label
    );
}

// ── HP3-5: DocClaim relocation → Valid ──────────────────────────────────────

/// T0: README has `` `column` `` inline-code mention at a specific byte offset.
/// Post commit: 26 lines are inserted above the mention; the mention text is
/// byte-identical but now lives at a higher byte offset.
///
/// **Why it fails on HEAD a6b7e5c:**
/// `matching_post_fact` for `DocClaim` reads the post-commit blob at the
/// ORIGINAL byte range stored in `mention_span`.  After the insertion, those
/// original offsets contain the newly-inserted lines, not the `` `column` ``
/// mention text — so the bytes differ, the hashes differ, and the rule engine
/// emits `StaleSourceChanged` instead of `Valid`.  The fix (in a later task)
/// must scan the post-commit markdown for the mention text rather than
/// anchoring to the original byte offset.
#[test]
fn hp3_5_doc_claim_relocation_is_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(p.join("src/lib.rs"), b"pub fn column() {}\n").unwrap();

    // Build a markdown where `column` appears after several filler lines so
    // inserting lines above it shifts its byte offset meaningfully.
    let mut md = String::new();
    for i in 1..=20 {
        md.push_str(&format!("Paragraph {}. Some documentation text here.\n", i));
    }
    md.push_str("Use `column` to retrieve the column value.\n");
    std::fs::write(p.join("README.md"), md.as_bytes()).unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // Insert 26 new lines above the existing content. The mention text is
    // byte-identical (`column`) but its byte offset has shifted by the total
    // byte count of the 26 inserted lines.
    let mut md2 = String::new();
    for i in 1..=26 {
        md2.push_str(&format!("Inserted line {}.\n", i));
    }
    md2.push_str(&md);
    std::fs::write(p.join("README.md"), md2.as_bytes()).unwrap();
    commit_all_with_date(p, "insert-lines", "2025-01-02T00:00:00Z");
    let post_sha = rev_parse_head(p);

    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    };
    let rows = Replay::run(&cfg).unwrap();

    let doc_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.fact_id.starts_with("DocClaim::column::"))
        .collect();

    assert!(
        !doc_rows.is_empty(),
        "test setup must emit at least one DocClaim::column row; \
         ensure the markdown mention resolves to the known symbol"
    );

    let post_row = doc_rows
        .iter()
        .find(|r| r.commit_sha == post_sha)
        .expect("post-T0 DocClaim::column row missing");

    assert!(
        matches!(post_row.label, Label::Valid),
        "DocClaim mention relocated by line insertion must be Valid (text \
         byte-identical, only offset changed), got {:?}",
        post_row.label
    );
}

// ── HP3-4: rename true positive (preservation) ──────────────────────────────

/// `fn locations_mut()` in `impl AutomataCaptures` becomes
/// `fn captures_mut()` in the same `impl AutomataCaptures`.
///
/// The two spans are byte-near-identical (same signature shape, same body
/// structure, only the function name differs).  The `similar`-based heuristic
/// (`TextDiff::ratio ≥ 0.6`) should fire and return
/// `StaleSymbolRenamed { new_name: "captures_mut" }`.
///
/// This test is GREEN on HEAD a6b7e5c and must remain GREEN through Task 4
/// (which will tighten the rename heuristic to fix the false-positive cases).
/// If it turns RED after Task 4's changes, Task 4 must document the regression
/// and either restore this test or add a SPEC §11 entry for the changed rule.
///
/// Note: rename detection requires `skip_symbol_resolution = false` in the
/// full replay loop, which in turn requires rust-analyzer.  This test
/// exercises the heuristic at the `diff::rename_candidate` unit level only,
/// where no rust-analyzer dependency exists.  See `diff.rs` for the
/// corresponding unit test.  This integration-level placeholder records the
/// preservation contract in the same file as the other HP3 cases.
#[test]
fn hp3_4_rename_true_positive_preservation_recorded_at_diff_level() {
    // Verify the same-container rename heuristic inline so this file contains
    // a runnable assertion alongside the other HP3 cases.
    //
    // Detailed fixture and rationale live in `diff.rs` as
    // `hp3_4_rename_true_positive_same_impl_is_detected`.
    //
    // Verified on HEAD a6b7e5c: ratio ≥ 0.6 fires for the
    // `locations_mut` → `captures_mut` same-container case.
    let before = b"fn locations_mut(&mut self) -> &mut Vec<Location> { &mut self.locations }";
    let candidates = vec![(
        "captures_mut".to_string(),
        b"fn captures_mut(&mut self) -> &mut Vec<Location> { &mut self.locations }".to_vec(),
    )];
    let result = rename_candidate(before, &candidates, 0.6);
    assert_eq!(
        result.as_deref(),
        Some("captures_mut"),
        "preservation: locations_mut → captures_mut must remain a detected rename"
    );
}

// ── HP3-4b: rename true positive through the PRODUCTION classify path ────────

/// Exercises the full production pipeline for a within-file function rename:
///   T₀  → `pub fn find_entry(key: &str) -> Option<u32> { None }`
///   C1  → `pub fn find_result(key: &str) -> Option<u32> { None }`
///           (old name deleted, new name added — same body, same module context)
///
/// This test uses `Replay::run_with_resolver` with `skip_symbol_resolution=false`
/// so the full path is exercised:
///   1. `CommitSymbolIndex` is built from C1's blob tree.
///   2. `matching_post_fact` returns `None` (old name absent in C1).
///   3. `CommitSymbolIndex::symbol_exists_in_tree` returns `false`
///      (old name is gone from every path in the tree).
///   4. The typed rename pipeline (`rename_candidates_for_typed` +
///      `rename_candidate_typed`) runs and detects `find_result` as the
///      rename target (shared `find_` prefix, high span similarity).
///   5. `classify` emits `StaleSymbolRenamed { new_name: "find_result" }`.
///
/// This is the integration-level gate for the Task 4 wiring: the typed
/// pipeline was previously only reachable via the dead fallback branch;
/// after this fix it runs in the live `CommitSymbolIndex` branch.
#[test]
fn hp3_4b_rename_through_production_classify_path() {
    struct NullResolver;
    impl SymbolResolver for NullResolver {
        fn resolve(&mut self, _name: &str) -> anyhow::Result<Option<ResolvedLocation>> {
            Ok(None)
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    // T0: only `find_entry` exists.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub fn find_entry(key: &str) -> Option<u32> { None }\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // C1: rename find_entry → find_result (same body, only name changed).
    // `find_result` is NOT present at T0, so Gate 2 passes.
    // Leaf-name similarity: "find_entry" vs "find_result"
    //   TextDiff::from_chars("find_entry", "find_result").ratio()
    //   Both share "find_" (6 chars), differ in "entry"(5) vs "result"(6).
    //   LCS ≈ 6 common + chars in "entr"/"resul" partial match → ratio > 0.6.
    // Span similarity: identical body except the name → ratio > 0.6.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub fn find_result(key: &str) -> Option<u32> { None }\n",
    )
    .unwrap();
    commit_all_with_date(
        p,
        "rename-find-entry-to-find-result",
        "2025-01-02T00:00:00Z",
    );
    let c1 = rev_parse_head(p);

    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: false,
    };
    let rows = Replay::run_with_resolver(&cfg, Some(Box::new(NullResolver))).unwrap();

    let fn_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.fact_id.contains("find_entry"))
        .collect();

    let c1_row = fn_rows
        .iter()
        .find(|r| r.commit_sha == c1)
        .expect("C1 row for find_entry missing");

    assert!(
        matches!(
            &c1_row.label,
            Label::StaleSymbolRenamed { new_name } if new_name == "find_result"
        ),
        "production rename path: find_entry → find_result must emit \
         StaleSymbolRenamed {{ new_name: \"find_result\" }}, got {:?}",
        c1_row.label
    );
}

// ── HP4-1: multi-assertion TestAssertion does NOT collapse to assertion #1 ───
//
// Pass 4 root cause: `match_post::matching_post_fact`'s `Fact::TestAssertion`
// arm calls `find_map(|f| q == *test_fn)`, which returns the FIRST assertion
// in the post-commit test fn for every T₀ fact in that fn. With N assertions
// in a single `#[test]` body, assertions 2..N always hash-mismatch against
// assertion #1's hash and route to `StaleSourceChanged`. See findings doc:
// `benchmarks/provbench/spotcheck/2026-05-12-post-pass3-findings.md`.
//
// This test builds a 2-commit repo where T₀ has ONE test fn with TWO
// distinct assertions, and C1 modifies the SAME file *outside* that test fn
// (so Task 5's byte-identical fast-path cannot mask the matcher bug). After
// Task 4, both assertion rows at C1 must classify as `Valid` because the
// assertion bytes inside the test fn are unchanged.
#[test]
fn hp4_test_assertion_multi_assertion_matches_each_assertion_by_ordinal() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    // T₀: exactly ONE #[test] fn with TWO distinct assertions.
    std::fs::write(
        p.join("src/lib.rs"),
        b"#[test]\nfn parses_two_cases() {\n    assert!(1 + 1 == 2);\n    assert_eq!(2 + 2, 4);\n}\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // C1: SAME file, modified OUTSIDE `parses_two_cases` (appends a helper).
    // The two assertion-macro bytes inside the test fn are unchanged.
    std::fs::write(
        p.join("src/lib.rs"),
        b"#[test]\nfn parses_two_cases() {\n    assert!(1 + 1 == 2);\n    assert_eq!(2 + 2, 4);\n}\n\nfn _unused_helper() -> u32 { 0 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "add helper", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    let rows = Replay::run(&ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();

    // Filter to C1 rows for the multi-assertion test fn only.
    let c1_assertions: Vec<&FactAtCommit> = rows
        .iter()
        .filter(|r| r.commit_sha == c1)
        .filter(|r| {
            r.fact_id
                .starts_with("TestAssertion::parses_two_cases::src/lib.rs::")
        })
        .collect();

    // Vacuous-pass guard: extraction must emit EXACTLY two TestAssertion
    // facts for `parses_two_cases` (one per `assert!` invocation). A
    // regression that produced zero or one would otherwise pass the
    // Valid-label check below trivially.
    assert_eq!(
        c1_assertions.len(),
        2,
        "expected 2 TestAssertion rows for parses_two_cases at C1, got {}: {c1_assertions:#?}",
        c1_assertions.len()
    );

    // Both assertion bytes are unchanged; both rows must classify Valid.
    // On the buggy HEAD (pre-Task-4): assertion #1 → Valid, assertion #2
    // → StaleSourceChanged because match_post returned assertion #1's hash
    // for both T₀ facts.
    for row in &c1_assertions {
        assert_eq!(
            row.label,
            Label::Valid,
            "every assertion in an unchanged test fn must classify Valid; got {row:?}"
        );
    }
}

// ── HP4-2: byte-identical source file ⇒ Valid for every fact at that path ───
//
// SPEC §5 structural invariant: when a fact's source path is byte-identical
// between T₀ and the replay commit, every fact anchored in that path is
// `Valid`. This test locks the invariant across ALL FIVE fact kinds
// (FunctionSignature, Field, PublicSymbol, DocClaim, TestAssertion) by
// constructing a fixture where each kind is exercised at T₀ and the C1
// commit touches only an *unrelated* file (so both the Rust source AND
// the markdown source are byte-identical at C1).
//
// The fixture deliberately includes structurally-ambiguous same-key
// `FunctionSignature` facts (two `impl` blocks both defining `fn shared`)
// — the per-fact matcher's first-match behavior can mis-pair these even
// today, which is exactly the class of bug Task 5's structural guardrail
// is designed to prevent. The test fails on HEAD for at least one
// ambiguous `FunctionSignature::shared` row AND for non-first
// `TestAssertion` rows; it must fully pass after Task 5 (and Task 4 for
// the multi-assertion subset).
#[test]
fn hp4_byte_identical_source_file_forces_all_path_facts_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    // T₀ `src/lib.rs` exercises FunctionSignature (two ambiguous `shared`),
    // Field (`pub struct S`), PublicSymbol (`pub fn solo`), and
    // TestAssertion (two distinct asserts in #[test] fn t).
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub struct A;\n\
          pub struct B;\n\
          impl A { pub fn shared(&self) -> u32 { 1 } }\n\
          impl B { pub fn shared(&self) -> u32 { 2 } }\n\
          \n\
          pub struct S { pub x: u32, pub y: u32 }\n\
          pub fn solo() {}\n\
          \n\
          #[test]\n\
          fn t() {\n\
              assert!(1 == 1);\n\
              assert_eq!(2, 2);\n\
          }\n",
    )
    .unwrap();
    // T₀ README mentions `solo` so the DocClaim extractor anchors to
    // PublicSymbol::solo and emits a Fact::DocClaim.
    std::fs::write(
        p.join("README.md"),
        b"Use the `solo` helper to do nothing.\n",
    )
    .unwrap();
    // Add an unrelated file we will modify at C1 so neither `src/lib.rs`
    // nor `README.md` changes.
    std::fs::write(p.join("OTHER.md"), b"unrelated content v1\n").unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // C1: modify ONLY OTHER.md. src/lib.rs and README.md are byte-identical
    // between T₀ and C1.
    std::fs::write(p.join("OTHER.md"), b"unrelated content v2\n").unwrap();
    commit_all_with_date(p, "tweak unrelated", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    let rows = Replay::run(&ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();

    // Vacuous-pass guard: at least one row of each of the five fact kinds
    // must be present in the C1 output. Without this, a future regression
    // that silently dropped a fact kind would pass the "all rows Valid"
    // assertion below trivially.
    let c1_rows: Vec<&FactAtCommit> = rows.iter().filter(|r| r.commit_sha == c1).collect();
    let kinds = [
        "FunctionSignature::",
        "Field::",
        "PublicSymbol::",
        "DocClaim::",
        "TestAssertion::",
    ];
    for kind in kinds {
        assert!(
            c1_rows.iter().any(|r| r.fact_id.starts_with(kind)),
            "expected at least one C1 row of kind `{kind}` (vacuous-pass guard); rows: {c1_rows:#?}"
        );
    }

    // Every C1 row anchored in src/lib.rs OR README.md must be Valid
    // (the structural SPEC §5 invariant). OTHER.md emits no facts so the
    // filter below is exhaustive for fact-source paths in this fixture.
    let mut violations: Vec<&FactAtCommit> = Vec::new();
    for row in &c1_rows {
        if (row.fact_id.contains("::src/lib.rs::") || row.fact_id.contains("::README.md::"))
            && row.label != Label::Valid
        {
            violations.push(row);
        }
    }
    assert!(
        violations.is_empty(),
        "SPEC §5 invariant: byte-identical file ⇒ Valid for every fact at that path; \
         found {} violation(s): {violations:#?}",
        violations.len()
    );
}

// ── HP4-3: modified assertion at same ordinal classifies StaleSourceChanged ─
//
// Locks in the ordinal-pairing contract from Task 4. Without ordinal
// pairing (e.g. a pure content-hash matcher), assertion #2 (whose bytes
// changed) would find no exact-hash match in the post-commit test fn,
// `matching_post_fact` would return `None`, and upstream routing would
// produce `NeedsRevalidation` (or `StaleSourceDeleted`) rather than the
// correct `StaleSourceChanged`. Ordinal pairing returns the post-commit
// assertion at the SAME index, whose hash differs from the T₀ hash,
// driving `structurally_classifiable = true` → `StaleSourceChanged`.
//
// Pre-Task-4: assertion #1 → Valid (find_map happens to land on the same
// fact); assertion #2 → StaleSourceChanged by accident (find_map returns
// post-#1 whose hash differs from T₀ #2's hash); assertion #3 →
// StaleSourceChanged WRONGLY (find_map returns post-#1 whose hash differs
// from T₀ #3's hash, but #3's bytes are unchanged in this fixture).
//
// Post-Task-4: #1 Valid, #2 StaleSourceChanged, #3 Valid.
#[test]
fn hp4_test_assertion_body_change_same_ordinal_is_stale_source_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    // T₀: ONE #[test] fn with THREE distinct assertions (distinct content
    // hashes). The line numbers in span are 3, 4, 5 inside the test body.
    std::fs::write(
        p.join("src/lib.rs"),
        b"#[test]\nfn t() {\n    assert!(1 == 1);\n    assert_eq!(2, 2);\n    assert_ne!(3, 4);\n}\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // C1: modify ONLY the bytes of assertion #2. It remains at position #2
    // in the test fn. File is NOT byte-identical, so Task 5's fast-path
    // does not mask the matcher behavior.
    std::fs::write(
        p.join("src/lib.rs"),
        b"#[test]\nfn t() {\n    assert!(1 == 1);\n    assert_eq!(2, 22);\n    assert_ne!(3, 4);\n}\n",
    )
    .unwrap();
    commit_all_with_date(p, "tweak assertion #2", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    let rows = Replay::run(&ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();

    // Collect C1 rows for test fn `t`, sorted by the line component of the
    // fact_id so we can address them by ordinal (#1, #2, #3 = lines 3, 4, 5).
    let mut c1_t_rows: Vec<&FactAtCommit> = rows
        .iter()
        .filter(|r| r.commit_sha == c1)
        .filter(|r| r.fact_id.starts_with("TestAssertion::t::src/lib.rs::"))
        .collect();
    c1_t_rows.sort_by_key(|r| {
        r.fact_id
            .rsplit("::")
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
    });
    assert_eq!(
        c1_t_rows.len(),
        3,
        "expected 3 TestAssertion rows for `t` at C1, got {}: {c1_t_rows:#?}",
        c1_t_rows.len()
    );

    assert_eq!(
        c1_t_rows[0].label,
        Label::Valid,
        "assertion #1 (unchanged) must classify Valid; got {:?}",
        c1_t_rows[0]
    );
    assert_eq!(
        c1_t_rows[1].label,
        Label::StaleSourceChanged,
        "assertion #2 (bytes modified) must classify StaleSourceChanged; got {:?}",
        c1_t_rows[1]
    );
    assert_eq!(
        c1_t_rows[2].label,
        Label::Valid,
        "assertion #3 (unchanged) must classify Valid; got {:?}",
        c1_t_rows[2]
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass-5 regressions
//
// Three disagreement clusters from the post-pass-4 SPEC §9.1 gate FAIL
// (sample-eaf82d2.csv, point estimate 93.00% / Wilson 88.59%):
//
//   E — cfg/impl-ambiguous FunctionSignature multi-def: find_map returns the
//       wrong same-qualified-name survivor when the T₀ fact's specific
//       cfg/impl variant is deleted. Mirror pass-4's TestAssertion ordinal
//       disambiguator but key on (qualified_name, cfg_set, impl_receiver_type)
//       with an ordinal tiebreaker.
//
//   G — PublicSymbol pub-use re-export form change: T₀ `pub fn X` → C1
//       `pub use … X` hashes mismatch but the public surface is preserved.
//       Bare pub-use re-exports + alias re-exports must classify Valid;
//       restricted-visibility uses (pub(crate) use, etc.) MUST remain
//       narrowed → StaleSourceChanged (pass-3 semantics preserved).
//
//   F — Field-out-of-named-container: field leaf appears in same file but
//       inside a different struct/variant. File-local check routes to
//       NeedsRevalidation; cross-file leaf tracking is explicitly out of
//       scope per the pass-5 final plan.
//
// Full evidence + per-cluster row tables:
// benchmarks/provbench/spotcheck/2026-05-13-post-pass4-findings.md (merged
// to main via PR #36).
// ─────────────────────────────────────────────────────────────────────────────

// ── HP5-1: FunctionSignature cfg-variant deletion routes to NeedsRevalidation

/// Pass-5 Cluster E: T₀ has THREE same-qualified-name `pub fn load(...)`
/// definitions guarded by distinct `#[cfg(...)]` attributes (unix, windows,
/// wasm32 placeholder). C1 deletes ONLY the wasm32 placeholder; the unix
/// and windows variants survive byte-identically. On the pre-fix labeler
/// the wasm32 row mis-pairs against the unix variant via `find_map` and
/// classifies StaleSourceChanged. After Task 2, the disambiguator
/// `(cfg_set, impl_receiver=None)` for the T₀ wasm32 fact finds no match
/// in the post AST → `matching_post_fact` returns `Ok(None)` → upstream
/// `commit_index.symbol_exists_in_tree` sees the qualified name surviving
/// in the unix/windows variants → routes to `NeedsRevalidation`.
///
/// Runs with `skip_symbol_resolution=false` so the commit_index path is
/// exercised; this is the production code path Cluster E hits.
#[test]
fn hp5_function_signature_cfg_variant_deletion_routes_needs_revalidation() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    // T₀: three cfg-gated `pub fn load` definitions, all sharing
    // qualified_name = "load".
    std::fs::write(
        p.join("src/lib.rs"),
        b"#[cfg(unix)]\npub fn load(p: &str) -> u32 { 1 }\n\
          #[cfg(windows)]\npub fn load(p: &str) -> u32 { 2 }\n\
          #[cfg(not(any(unix, windows)))]\npub fn load(p: &str) -> u32 { 3 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // C1: delete ONLY the wasm32 placeholder. unix and windows variants
    // are byte-identical at C1; file as a whole is NOT byte-identical
    // (so the pass-4 byte-identical fast-path doesn't mask the bug).
    std::fs::write(
        p.join("src/lib.rs"),
        b"#[cfg(unix)]\npub fn load(p: &str) -> u32 { 1 }\n\
          #[cfg(windows)]\npub fn load(p: &str) -> u32 { 2 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "drop wasm32 placeholder", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    // Run with skip_symbol_resolution = false so commit_index engages.
    // Use Replay::run_with_resolver (the production code path) with a
    // null resolver — pass-3's hp3_2b proved this exercises the
    // commit-tree-local index without an RA dependency.
    struct NullResolver;
    impl SymbolResolver for NullResolver {
        fn resolve(&mut self, _name: &str) -> anyhow::Result<Option<ResolvedLocation>> {
            Ok(None)
        }
    }
    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: false,
    };
    let rows = Replay::run_with_resolver(&cfg, Some(Box::new(NullResolver))).unwrap();

    // Vacuous-pass guard: T₀ extraction must produce THREE
    // FunctionSignature::load rows (one per cfg variant).
    let t0_load_rows: Vec<&FactAtCommit> = rows
        .iter()
        .filter(|r| r.commit_sha == c1)
        .filter(|r| r.fact_id.starts_with("FunctionSignature::load::src/lib.rs::"))
        .collect();
    assert_eq!(
        t0_load_rows.len(),
        3,
        "expected 3 FunctionSignature::load rows at C1 (one per cfg variant); got {}: {t0_load_rows:#?}",
        t0_load_rows.len(),
    );

    // The wasm32 variant's T₀ line is the third `pub fn load` declaration,
    // emitted at the line where `#[cfg(not(any(unix, windows)))]` starts.
    // In the T₀ fixture above that line is 5. We identify the wasm32 row
    // by its line component.
    let wasm32_row = t0_load_rows
        .iter()
        .find(|r| r.fact_id.ends_with("::5"))
        .expect("wasm32 placeholder row at line 5 missing");
    assert_eq!(
        wasm32_row.label,
        Label::NeedsRevalidation,
        "wasm32-cfg `load` deleted while unix/windows survivors exist must \
         classify NeedsRevalidation; got {:?}",
        wasm32_row.label
    );

    // The unix and windows variants are byte-identical at C1 so they
    // must classify Valid (file changed → fast-path doesn't apply, so
    // the disambiguator's exact-match path returns the same span+hash).
    for line in ["1", "3"] {
        let suffix = format!("::{line}");
        let row = t0_load_rows
            .iter()
            .find(|r| r.fact_id.ends_with(&suffix))
            .unwrap_or_else(|| panic!("missing FunctionSignature::load row at line {line}"));
        assert_eq!(
            row.label,
            Label::Valid,
            "unchanged cfg variant at line {line} must classify Valid; got {:?}",
            row.label
        );
    }
}

// ── HP5-2: FunctionSignature impl-receiver disambiguates same method name ───

/// Pass-5 Cluster E: T₀ has `impl A { pub fn shared() -> u32 { 1 } }` and
/// `impl B { pub fn shared() -> u32 { 2 } }`. Both extract to
/// `qualified_name = "shared"` because the current extractor module-
/// qualifies functions but does NOT include impl-receiver context. C1
/// changes the SIGNATURE of `impl A::shared` (return type → `-> u64`) —
/// NOT the body — so the bytes inside the FunctionSignature span
/// (which stops before the body brace per `function_signature.rs:97-107`)
/// produce a hash mismatch. `impl B::shared` is unchanged.
///
/// On the pre-fix labeler, find_map returns the first `shared` it sees
/// at C1 — the changed impl-A signature — for BOTH T₀ facts, mis-pairing
/// impl B with A's new hash → both flagged stale.
///
/// After Task 2, the disambiguator `(cfg_set={}, impl_receiver=Some("A"))`
/// for T₀ fact #1 matches only the C1 impl-A signature → hash mismatch
/// → StaleSourceChanged (correct). The disambiguator
/// `(cfg_set={}, impl_receiver=Some("B"))` for T₀ fact #2 matches only
/// the C1 impl-B signature → hash unchanged → Valid (correct).
#[test]
fn hp5_function_signature_impl_receiver_disambiguates_same_method_name() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    // T₀: two `impl` blocks each with `pub fn shared() -> u32`.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub struct A;\npub struct B;\n\
          impl A {\n    pub fn shared(&self) -> u32 { 1 }\n}\n\
          impl B {\n    pub fn shared(&self) -> u32 { 2 }\n}\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // C1: change ONLY the signature (return type) of impl A::shared.
    // Body bytes change as well (literal 1 stays), but the
    // FunctionSignature span byte_range covers up through the `-> u32`
    // or `-> u64` token — modifying the return type is what guarantees
    // the content_hash changes. impl B::shared is unchanged.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub struct A;\npub struct B;\n\
          impl A {\n    pub fn shared(&self) -> u64 { 1 }\n}\n\
          impl B {\n    pub fn shared(&self) -> u32 { 2 }\n}\n",
    )
    .unwrap();
    commit_all_with_date(p, "widen impl A return", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    let rows = Replay::run(&ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();

    let c1_rows: Vec<&FactAtCommit> = rows
        .iter()
        .filter(|r| r.commit_sha == c1)
        .filter(|r| r.fact_id.starts_with("FunctionSignature::shared::src/lib.rs::"))
        .collect();
    assert_eq!(
        c1_rows.len(),
        2,
        "expected 2 FunctionSignature::shared rows (one per impl); got {}: {c1_rows:#?}",
        c1_rows.len(),
    );

    // The T₀ impl-A `shared` is at line 4 (line of `impl A {` ... `fn shared`
    // attribute-or-self span begins). impl-B `shared` is later (line 7-ish).
    // Sort by the line component to address them as [A, B].
    let mut sorted: Vec<&FactAtCommit> = c1_rows.clone();
    sorted.sort_by_key(|r| {
        r.fact_id
            .rsplit("::")
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
    });

    assert_eq!(
        sorted[0].label,
        Label::StaleSourceChanged,
        "impl A::shared signature changed (return type widened) must classify \
         StaleSourceChanged; got {:?}",
        sorted[0]
    );
    assert_eq!(
        sorted[1].label,
        Label::Valid,
        "impl B::shared unchanged must classify Valid; got {:?}",
        sorted[1]
    );
}

// ── HP5-3: PublicSymbol pub-use re-export is Valid ──────────────────────────

/// Pass-5 Cluster G: T₀ has `pub fn pattern() {}` at the top level of
/// `src/lib.rs`. C1 deletes the pub fn and replaces it with
/// `mod inner; pub use crate::inner::pattern;` (with `mod inner;`
/// referencing a sibling `src/inner.rs` that defines the actual pub
/// fn). The public symbol `pattern` is still exported from the crate
/// top-level via the bare pub-use, but the form changed from a direct
/// definition to a re-export. `symbol_existence::extract` for
/// `src/lib.rs` at C1 emits a PublicSymbol with `qualified_name =
/// "pattern"` from the `pub use` declaration, but its span (the full
/// use_declaration) hashes differently than T₀'s `pub fn` declaration
/// span → `StaleSourceChanged` on the pre-fix labeler.
///
/// **Fixture note (intentional sibling file):** we use a sibling
/// `src/inner.rs` rather than `mod inner { pub fn pattern() {} }`
/// inline because the labeler's bare-name PublicSymbol matching would
/// otherwise find the nested `pub fn pattern` and return its
/// `(span, hash)` — which happens to hash identically to T₀'s
/// top-level `pub fn pattern` because `extract_named_item` spans only
/// `pub fn name` (no module prefix). That accidental hash-match would
/// let the test pass on base for the wrong reason. A sibling file
/// isolates the C1 lib.rs AST so the only `qualified_name = "pattern"`
/// candidate at that path is the `pub use` itself.
///
/// After Task 3, the form-aware lookup detects the bare pub-use form
/// for the same exported name and returns `(bare_pub_use_post_span,
/// observed_hash.clone())` → `structural` evaluates `false` → Valid.
#[test]
fn hp5_public_symbol_pub_use_reexport_is_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    // T₀: pub fn pattern at top level of lib.rs; no sibling file.
    std::fs::write(p.join("src/lib.rs"), b"pub fn pattern() -> u32 { 1 }\n").unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // C1: replace top-level pub fn with `mod inner; pub use ...` and add
    // a sibling src/inner.rs holding the actual pub fn.
    std::fs::write(
        p.join("src/lib.rs"),
        b"mod inner;\npub use crate::inner::pattern;\n",
    )
    .unwrap();
    std::fs::write(p.join("src/inner.rs"), b"pub fn pattern() -> u32 { 1 }\n").unwrap();
    commit_all_with_date(p, "reexport pattern via pub use", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    let rows = Replay::run(&ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();

    // The T₀ fact's fact_id is anchored at T₀ line 1.
    let row = rows
        .iter()
        .find(|r| r.commit_sha == c1 && r.fact_id == "PublicSymbol::pattern::src/lib.rs::1")
        .expect("PublicSymbol::pattern T₀ row at C1 missing");
    assert_eq!(
        row.label,
        Label::Valid,
        "bare `pub use crate::inner::pattern;` preserves the exported \
         name `pattern` → public-surface continuity → Valid; got {:?}",
        row.label
    );
}

// ── HP5-4: PublicSymbol pub-use alias re-export is Valid ────────────────────

/// Pass-5 Cluster G: same shape as hp5_3 but the re-export renames
/// via `as`. T₀ has `pub fn pattern() {}` at the top level of lib.rs.
/// C1 has `mod inner; pub use crate::inner::original_pattern as
/// pattern;` with `src/inner.rs` defining `pub fn original_pattern`.
/// The exported name `pattern` is preserved despite the underlying
/// definition now having a different identifier.
#[test]
fn hp5_public_symbol_pub_use_alias_reexport_is_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(p.join("src/lib.rs"), b"pub fn pattern() -> u32 { 1 }\n").unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    std::fs::write(
        p.join("src/lib.rs"),
        b"mod inner;\npub use crate::inner::original_pattern as pattern;\n",
    )
    .unwrap();
    std::fs::write(
        p.join("src/inner.rs"),
        b"pub fn original_pattern() -> u32 { 1 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "reexport via alias", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    let rows = Replay::run(&ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();

    let row = rows
        .iter()
        .find(|r| r.commit_sha == c1 && r.fact_id == "PublicSymbol::pattern::src/lib.rs::1")
        .expect("PublicSymbol::pattern T₀ row at C1 missing");
    assert_eq!(
        row.label,
        Label::Valid,
        "`pub use crate::inner::original_pattern as pattern;` preserves \
         the exported name `pattern` → Valid; got {:?}",
        row.label
    );
}

// ── HP5-5: Field moved to nested struct routes to NeedsRevalidation ─────────

/// Pass-5 Cluster F: T₀ has `pub struct Config { pub dfa_size_limit:
/// usize, … }`. C1 restructures into `pub struct Config { pub inner:
/// ConfigInner } pub struct ConfigInner { pub dfa_size_limit: usize }` —
/// the field leaf `dfa_size_limit` still exists in the same file but
/// inside a different parent struct.
///
/// The labeler's exact-match `qualified_path = "Config::dfa_size_limit"`
/// no longer resolves at C1. After Task 4, the file-local helper
/// `field::same_file_leaf_elsewhere` detects that a Fact::Field with leaf
/// `dfa_size_limit` exists at a different qualified_path → routes to
/// NeedsRevalidation (gray area for LLM follow-up review).
#[test]
fn hp5_field_leaf_moved_to_nested_struct_routes_needs_revalidation() {
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
        b"pub struct Config {\n    pub dfa_size_limit: usize,\n    pub other: u32,\n}\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    std::fs::write(
        p.join("src/lib.rs"),
        b"pub struct Config {\n    pub inner: ConfigInner,\n}\n\
          pub struct ConfigInner {\n    pub dfa_size_limit: usize,\n    pub other: u32,\n}\n",
    )
    .unwrap();
    commit_all_with_date(p, "nest fields into ConfigInner", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    let rows = Replay::run(&ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();

    let row = rows
        .iter()
        .find(|r| {
            r.commit_sha == c1 && r.fact_id.starts_with("Field::Config::dfa_size_limit::")
        })
        .expect("Field::Config::dfa_size_limit T₀ row at C1 missing");
    assert_eq!(
        row.label,
        Label::NeedsRevalidation,
        "field leaf `dfa_size_limit` moved into nested struct ConfigInner \
         (same file, same leaf, different container) must route \
         NeedsRevalidation; got {:?}",
        row.label
    );
}

// ── HP5-6: Field moved to enum variant routes to NeedsRevalidation ──────────

/// Pass-5 Cluster F variant: T₀ has `pub struct SinkContext { pub kind:
/// u32 }`. C1 converts `SinkContext` into a struct-style enum where
/// each variant carries a `kind` field. The leaf `kind` survives in
/// same file but only as a per-variant struct field — the
/// `Field::SinkContext::kind` qualified_path no longer exists as a
/// top-level struct field.
#[test]
fn hp5_field_leaf_moved_to_enum_variant_routes_needs_revalidation() {
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
        b"pub struct SinkContext {\n    pub kind: u32,\n    pub other: u32,\n}\n",
    )
    .unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    std::fs::write(
        p.join("src/lib.rs"),
        b"pub enum SinkContext {\n    Match { kind: u32, other: u32 },\n    Context { kind: u32, other: u32 },\n}\n",
    )
    .unwrap();
    commit_all_with_date(p, "convert SinkContext to enum", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    let rows = Replay::run(&ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();

    let row = rows
        .iter()
        .find(|r| r.commit_sha == c1 && r.fact_id.starts_with("Field::SinkContext::kind::"))
        .expect("Field::SinkContext::kind T₀ row at C1 missing");
    assert_eq!(
        row.label,
        Label::NeedsRevalidation,
        "field leaf `kind` moved into enum-variant struct fields (same \
         file, same leaf, different container kind) must route \
         NeedsRevalidation; got {:?}",
        row.label
    );
}

// ── HP5-preservation: pub(crate) direct narrowing must remain stale ────────

/// Pass-5 preservation: direct visibility narrowing
/// `pub fn pattern` → `pub(crate) fn pattern` (same path, same name,
/// narrowed visibility, no `use`-declaration) must continue to classify
/// `StaleSourceChanged` via the existing pass-3 narrowing logic. Task 3's
/// `pub use` form-aware path must NOT over-promote this case to `Valid`.
///
/// Note: pass-3 covers same-path-narrowing of named items. Narrowing
/// via `pub(crate) use` (re-export under restricted visibility) is a
/// separate case currently classified by the labeler as
/// `StaleSourceDeleted` or `Valid` depending on whether an underlying
/// `pub` fn of the same name is found anywhere in the file's mod tree.
/// That `pub(crate) use` case is structurally distinct from this
/// preservation test and is explicitly OUT of pass-5 scope.
///
/// MUST be GREEN on base `f26483d` and stay GREEN after Tasks 2-4.
#[test]
fn hp5_pub_crate_direct_narrowing_is_stale_source_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(p.join("src/lib.rs"), b"pub fn pattern() -> u32 { 1 }\n").unwrap();
    commit_all_with_date(p, "init", "2025-01-01T00:00:00Z");
    let t0 = rev_parse_head(p);

    // C1: narrow `pattern` directly to pub(crate) at the same path.
    // Pass-3's visibility-narrowing logic handles this case →
    // StaleSourceChanged.
    std::fs::write(
        p.join("src/lib.rs"),
        b"pub(crate) fn pattern() -> u32 { 1 }\n",
    )
    .unwrap();
    commit_all_with_date(p, "narrow pattern to pub(crate)", "2025-01-02T00:00:00Z");
    let c1 = rev_parse_head(p);

    let rows = Replay::run(&ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    })
    .unwrap();

    let row = rows
        .iter()
        .find(|r| r.commit_sha == c1 && r.fact_id == "PublicSymbol::pattern::src/lib.rs::1")
        .expect("PublicSymbol::pattern T₀ row at C1 missing");
    assert_eq!(
        row.label,
        Label::StaleSourceChanged,
        "`pub fn pattern` → `pub(crate) fn pattern` is direct visibility \
         narrowing → must classify StaleSourceChanged (pass-3 contract); \
         got {:?}",
        row.label
    );
}
