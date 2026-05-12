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
#[ignore = "RED until Task 5 (doc-claim-relocation cluster)"]
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
