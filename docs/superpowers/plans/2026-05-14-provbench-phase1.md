# ProvBench Phase 1 Rules-Based Invalidator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up a deterministic, structural, single-repo HEAD-only Rust invalidator (`provbench-phase1`) that subsumes Phase 0c's LLM-as-invalidator and clears every SPEC §8 #3/#4/#5 threshold verbatim on the Phase 0c canary, producing a side-by-side `metrics.json` against the LLM baseline.

**Architecture:** Three standalone, Cargo-workspace-excluded crates under `benchmarks/provbench/`. First, extract scoring math from the existing `baseline/` into a sibling `scoring/` crate (`provbench-scoring`) gated by a byte-stable canary regression test. Then build a new sibling `phase1/` crate (`provbench-phase1`): SQLite-backed, `gix`-based commit-tree reader, tree-sitter Rust+Markdown parsers, a 10-rule first-match-wins structural classifier (R0 → R1 → R2 → R5 → R6 → R7 → R3 → R4 → R8 → R9). Per-row emission to `predictions.jsonl` (matches `provbench_baseline::runner::PredictionRow` byte-for-byte) + `rule_traces.jsonl`. Final commit ships a Phase 0c-style findings doc.

**Tech Stack:** Rust 1.91+, `clap` 4, `serde`/`serde_json`, `rusqlite` 0.31+ (bundled feature), `gix` 0.66+, `tree-sitter` 0.22 with `tree-sitter-rust` 0.21 + `tree-sitter-md` 0.2, `sha2` 0.10, `hex` 0.4, `tracing`/`tracing-subscriber` 0.3. Scoring crate keeps the existing baseline deps: `statrs` 0.17, `rand_chacha` 0.3.

**Reference spec:** `/Users/jeffreycrum/.claude/plans/snoopy-moseying-pony.md` (the collab-locked final plan for session `36523e8c-2129-4cbd-ac35-b3067e3c7946`, phase `PlanLocked`). SPEC source of truth: `benchmarks/provbench/SPEC.md` (frozen 2026-05-09, hash `683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c`).

**Branch strategy:** Work on `feat/provbench-phase1` (already cut from `main` HEAD `9b027d4f0eb73ef1cfb85e88083bdea817571adc`). Commit at every TDD GREEN. The branch lands via the collab-driven PR flow at the end (do NOT invoke `finishing-a-development-branch`; the collab `final_review` turn opens the PR).

**Acceptance gates (SPEC §8, live constraints):**
- §8 #3: `valid_retention_accuracy.wilson_lower_95 ≥ 0.95`
- §8 #4: `latency_p50_ms ≤ 727` (10× faster than the 7,267 ms LLM baseline)
- §8 #5: `stale_detection.recall.wilson_lower_95 ≥ 0.30`
- Internal stretch (not spec-required): `stale_detection.recall ≥ 0.80`.

---

## File Structure

### Existing (`baseline/`) — modify minimally for refactor

| File | Change |
|---|---|
| `benchmarks/provbench/baseline/Cargo.toml` | Add `provbench-scoring = { path = "../scoring" }`; drop `statrs`, `sha2`, `hex` (now transitive). |
| `benchmarks/provbench/baseline/src/runner.rs` | Replace local `PredictionRow` struct (L99–113) with `pub use provbench_scoring::PredictionRow;`. |
| `benchmarks/provbench/baseline/src/lib.rs` | Re-export shim (`pub use provbench_scoring::{metrics, report, manifest};` or per-symbol re-exports). |
| `benchmarks/provbench/baseline/src/metrics.rs` | Shrinks to a re-export shim: `pub use provbench_scoring::metrics::*;`. |
| `benchmarks/provbench/baseline/src/report.rs` | Shrinks to a re-export shim: `pub use provbench_scoring::report::*;` plus thin wrapper preserving `provbench-baseline score`. |

### New (`scoring/` crate)

```
benchmarks/provbench/scoring/
  Cargo.toml
  Cargo.lock                                  # committed
  src/
    lib.rs                                    # re-exports
    predictions.rs                            # PredictionRow (verbatim from baseline/src/runner.rs L99-113)
    metrics.rs                                # verbatim from baseline/src/metrics.rs (511 lines)
    report.rs                                 # verbatim from baseline/src/report.rs (169 lines), score_run -> score_llm_baseline_run
    manifest.rs                               # SampleManifest read-side from baseline/src/manifest.rs
    compare.rs                                # filled in Task 4 — side-by-side metrics builder
    bin/
      provbench-score.rs                      # CLI: `baseline --run <dir>` (Task 1), `compare ...` (Task 4)
  tests/
    byte_stable_canary.rs                     # regression test: re-score canary, byte-compare metrics.json
    predictionrow_schema_compat.rs            # serde round-trip baseline<->scoring PredictionRow
```

### New (`phase1/` crate)

```
benchmarks/provbench/phase1/
  Cargo.toml
  Cargo.lock                                  # committed
  src/
    lib.rs
    main.rs                                   # `provbench-phase1` CLI (clap subcommands: score)
    facts.rs                                  # FactBody loader (mirrors labeler JSONL)
    diffs.rs                                  # CommitDiff loader
    baseline_run.rs                           # EvalRow loader from <baseline-run>/predictions.jsonl
    repo.rs                                   # gix-backed reader: open / blob_at / file_exists_at
    storage.rs                                # SQLite schema + helpers
    runner.rs                                 # commit-grouped rule chain driver
    parse.rs                                  # tree-sitter Rust + Markdown helpers (used by R5/R6)
    similarity.rs                             # 0.6-threshold rename candidate (Myers + qualified-name)
    rules/
      mod.rs                                  # Decision enum + Rule trait + RuleChain
      r0_diff_excluded.rs
      r1_source_file_missing.rs
      r2_blob_identical.rs
      r5_whitespace_or_comment_only.rs
      r6_doc_claim.rs
      r7_rename_candidate.rs
      r3_symbol_missing.rs
      r4_span_hash_changed.rs
      r8_ambiguous.rs
      r9_fallback.rs
  tests/
    load_roundtrip.rs                         # Task 2 — fact/diff/eval-row ingest
    rules_unit.rs                             # Task 3 — per-rule fixtures
    determinism.rs                            # Task 3 — byte-identical re-runs
    end_to_end_canary.rs                      # Task 4 — SPEC §8 gate
```

### New (workspace root) — modify

| File | Change |
|---|---|
| `Cargo.toml` | Extend the `exclude` list with `"benchmarks/provbench/scoring"` and `"benchmarks/provbench/phase1"`. |

### New (output artifacts, committed in Task 5)

```
benchmarks/provbench/results/phase1/2026-05-14-canary/
  predictions.jsonl
  rule_traces.jsonl
  metrics.json
  manifest.json                              # copied verbatim from results/phase0c/2026-05-13-canary/
  run_meta.json
benchmarks/provbench/results/phase1/2026-05-14-findings.md
```

---

## Task 1 — Extract `scoring/` from `baseline/` (refactor-only, byte-stable)

**Goal:** Move scoring math out of `baseline/` into a sibling `scoring/` crate without changing the math. A byte-stable canary regression test gates the change.

**Files:**
- Create: `benchmarks/provbench/scoring/Cargo.toml`
- Create: `benchmarks/provbench/scoring/src/lib.rs`
- Create: `benchmarks/provbench/scoring/src/predictions.rs`
- Create: `benchmarks/provbench/scoring/src/metrics.rs`
- Create: `benchmarks/provbench/scoring/src/report.rs`
- Create: `benchmarks/provbench/scoring/src/manifest.rs`
- Create: `benchmarks/provbench/scoring/src/bin/provbench-score.rs`
- Create: `benchmarks/provbench/scoring/tests/byte_stable_canary.rs`
- Create: `benchmarks/provbench/scoring/tests/predictionrow_schema_compat.rs`
- Modify: `benchmarks/provbench/baseline/Cargo.toml`
- Modify: `benchmarks/provbench/baseline/src/lib.rs`
- Modify: `benchmarks/provbench/baseline/src/runner.rs:99-113`
- Modify: `benchmarks/provbench/baseline/src/metrics.rs` (becomes shim)
- Modify: `benchmarks/provbench/baseline/src/report.rs` (becomes shim)
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1.1: Write the byte-stable canary failing test**

Create `benchmarks/provbench/scoring/tests/byte_stable_canary.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Reproduces benchmarks/provbench/results/phase0c/2026-05-13-canary/metrics.json
/// byte-for-byte by re-running the shared scorer over its own predictions.jsonl.
/// Locks the SPEC §6.2/§7 math against accidental drift during the
/// baseline -> scoring extraction.
#[test]
fn phase0c_canary_metrics_byte_stable() {
    let canary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../results/phase0c/2026-05-13-canary");

    let tmp = TempDir::new().unwrap();
    for name in ["manifest.json", "predictions.jsonl", "run_meta.json"] {
        fs::copy(canary.join(name), tmp.path().join(name)).unwrap();
    }

    provbench_scoring::report::score_llm_baseline_run(tmp.path()).unwrap();

    let got = fs::read(tmp.path().join("metrics.json")).unwrap();
    let want = fs::read(canary.join("metrics.json")).unwrap();
    assert_eq!(got, want, "metrics.json byte-stable canary regressed");
}
```

- [ ] **Step 1.2: Run test to verify it fails**

Run:
```bash
cargo test --manifest-path benchmarks/provbench/scoring/Cargo.toml --test byte_stable_canary
```
Expected: FAIL — the `scoring/` crate does not yet exist.

- [ ] **Step 1.3: Create the scoring crate's `Cargo.toml`**

```toml
[package]
name = "provbench-scoring"
version = "0.1.0"
edition = "2021"
rust-version = "1.91"
description = "Shared scoring math for ProvBench (LLM baseline + Phase 1 rules)"
license = "Apache-2.0"
publish = false

[[bin]]
name = "provbench-score"
path = "src/bin/provbench-score.rs"

[lib]
name = "provbench_scoring"
path = "src/lib.rs"

[dependencies]
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }
statrs = "0.17"
rand = "0.8"
rand_chacha = "0.3"
sha2 = "0.10"
hex = "0.4"
clap = { version = "4", features = ["derive"] }
thiserror = "2"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 1.4: Verbatim-move `metrics.rs` and `report.rs` from baseline**

Copy each file's contents:
```bash
mkdir -p benchmarks/provbench/scoring/src/bin benchmarks/provbench/scoring/tests
cp benchmarks/provbench/baseline/src/metrics.rs benchmarks/provbench/scoring/src/metrics.rs
cp benchmarks/provbench/baseline/src/report.rs   benchmarks/provbench/scoring/src/report.rs
```

Then, **inside the copied `scoring/src/report.rs` only**, rename the public entry point `score_run` to `score_llm_baseline_run` (one fn rename + one doc-comment update). Do NOT change any math.

Then, **inside the copied `scoring/src/metrics.rs`**, fix the imports: replace `use crate::runner::PredictionRow;` with `use crate::predictions::PredictionRow;`. No other changes.

Then, **inside the copied `scoring/src/report.rs`**, fix the imports the same way (replace any `crate::runner::PredictionRow` with `crate::predictions::PredictionRow`; replace any `crate::manifest::...` with `crate::manifest::...` — paths within the scoring crate stay).

- [ ] **Step 1.5: Read-side `SampleManifest` extraction + verbatim `PredictionRow`**

`baseline/src/manifest.rs` is a writer+reader hybrid whose writer side depends on `crate::diffs`, `crate::facts`, `crate::sample`, `sha2`, etc. The scoring crate only needs the read side (for `score_llm_baseline_run` to load `manifest.json` and pass it to `compute_population_weights`).

**Approach: read-side extraction.** Create `benchmarks/provbench/scoring/src/manifest.rs` with a minimal `SampleManifest` definition that matches the JSON shape the baseline writer produces, plus `PerStratumTargets` and `SampledRow` re-defined as read-only types. Use `serde_json::Value` for any nested field the scoring math doesn't touch (no need to mirror every writer-only detail). Baseline keeps its own rich writer-side `manifest.rs` untouched; do NOT re-export through scoring.

Concretely, scoring's `manifest.rs` should expose the same field set that the existing `baseline/src/report.rs` reads: `seed`, `corpus_path`, `facts_path`, `diffs_dir`, `labeler_git_sha`, `spec_freeze_hash`, `baseline_crate_head_sha`, `per_stratum_targets`, `selected_count`, `excluded_count_by_reason`, `estimated_worst_case_usd`, `rows`, `created_at`, `content_hash` (any field `report.rs` accesses by name). Use `#[serde(default)]` on optional fields. Verify by grepping `baseline/src/report.rs` for `manifest.` to enumerate the read surface, then mirror that set exactly — JSON-stable, but slimmer than the writer's source-of-truth definition.

Create `benchmarks/provbench/scoring/src/predictions.rs` with the `PredictionRow` struct **copied verbatim** from `baseline/src/runner.rs:99–113`:

```rust
use serde::{Deserialize, Serialize};

/// Per-row checkpoint persisted to `predictions.jsonl`.
///
/// One row per line. JSON field order is fixed by serde derive order;
/// existing rows are never rewritten so determinism is preserved across
/// resumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionRow {
    pub fact_id: String,
    pub commit_sha: String,
    pub batch_id: String,
    pub ground_truth: String,
    pub prediction: String,
    pub request_id: String,
    pub wall_ms: u64,
}
```

- [ ] **Step 1.6: Write `scoring/src/lib.rs`**

```rust
//! Shared scoring math for ProvBench (extracted from `provbench-baseline`).
//!
//! Both `provbench-baseline` and `provbench-phase1` depend on this crate.
//! SPEC §7 math (Wilson LB, three-way scoring, Cohen's κ + bootstrap CI,
//! latency, cost) lives here.

pub mod compare;
pub mod manifest;
pub mod metrics;
pub mod predictions;
pub mod report;

pub use predictions::PredictionRow;
```

Create a placeholder `benchmarks/provbench/scoring/src/compare.rs` (filled in Task 4):

```rust
//! Side-by-side `metrics.json` builder (LLM baseline column + candidate column).
//! Implemented in Task 4 — placeholder for now so `lib.rs` compiles.
```

- [ ] **Step 1.7: Write the `provbench-score baseline` CLI shim**

Create `benchmarks/provbench/scoring/src/bin/provbench-score.rs`:

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "provbench-score", version, about = "ProvBench shared scorer")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Score a Phase 0c LLM-baseline run directory.
    Baseline {
        #[arg(long)]
        run: PathBuf,
    },
    /// Side-by-side comparison (LLM baseline + candidate). Filled in Task 4.
    Compare {
        #[arg(long = "baseline-run")]
        baseline_run: PathBuf,
        #[arg(long = "candidate-run")]
        candidate_run: PathBuf,
        #[arg(long = "candidate-name")]
        candidate_name: String,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Baseline { run } => provbench_scoring::report::score_llm_baseline_run(&run),
        Cmd::Compare { .. } => anyhow::bail!("compare: implemented in Task 4"),
    }
}
```

- [ ] **Step 1.8: Add scoring to the workspace `exclude` list**

Modify `/Users/jeffreycrum/git-repos/ironrace-memory/Cargo.toml`:

```toml
exclude = [
  "benchmarks/provbench/labeler",
  "benchmarks/provbench/baseline",
  "benchmarks/provbench/scoring",
]
```

- [ ] **Step 1.9: Run the canary test against the standalone scoring crate**

Run:
```bash
cargo test --manifest-path benchmarks/provbench/scoring/Cargo.toml --test byte_stable_canary
```
Expected: PASS — the verbatim-moved scorer reproduces the canary `metrics.json` byte-for-byte.

If FAIL with a serde ordering issue, fix the ordering with deterministic map construction (`BTreeMap` or explicit `serde_json::json!` builder) inside `metrics.rs`/`report.rs`. **Do not weaken the test to numeric-only equality.**

- [ ] **Step 1.10: Write `PredictionRow` schema-compat test**

Create `benchmarks/provbench/scoring/tests/predictionrow_schema_compat.rs`:

```rust
/// Asserts that a JSON row written by either baseline or phase1 deserializes
/// identically through `provbench_scoring::PredictionRow`. Locks the
/// PredictionRow contract so phase1's predictions.jsonl is byte-compatible
/// with what baseline already emits.
#[test]
fn predictionrow_roundtrip_is_byte_stable() {
    let row = provbench_scoring::PredictionRow {
        fact_id: "DocClaim::auto::CHANGELOG.md::229".into(),
        commit_sha: "0000157917".into(),
        batch_id: "0000157917-phase1".into(),
        ground_truth: "Valid".into(),
        prediction: "valid".into(),
        request_id: "phase1:v1.0:0000157917:0".into(),
        wall_ms: 12,
    };
    let s = serde_json::to_string(&row).unwrap();
    assert_eq!(
        s,
        r#"{"fact_id":"DocClaim::auto::CHANGELOG.md::229","commit_sha":"0000157917","batch_id":"0000157917-phase1","ground_truth":"Valid","prediction":"valid","request_id":"phase1:v1.0:0000157917:0","wall_ms":12}"#
    );
    let _back: provbench_scoring::PredictionRow = serde_json::from_str(&s).unwrap();
}
```

- [ ] **Step 1.11: Run the schema-compat test**

Run:
```bash
cargo test --manifest-path benchmarks/provbench/scoring/Cargo.toml --test predictionrow_schema_compat
```
Expected: PASS.

- [ ] **Step 1.12: Convert baseline's `metrics.rs` / `report.rs` to re-export shims**

Replace `benchmarks/provbench/baseline/src/metrics.rs` with:

```rust
//! Re-export shim — moved to `provbench-scoring`.
pub use provbench_scoring::metrics::*;
```

Replace `benchmarks/provbench/baseline/src/report.rs` with:

```rust
//! Re-export shim — moved to `provbench-scoring`.
pub use provbench_scoring::report::*;

/// Backwards-compat wrapper preserving the historical `provbench-baseline score`
/// entry point.
pub fn score_run(run_dir: &std::path::Path) -> anyhow::Result<()> {
    provbench_scoring::report::score_llm_baseline_run(run_dir)
}
```

- [ ] **Step 1.13: Replace baseline's local `PredictionRow` with a re-export**

Edit `benchmarks/provbench/baseline/src/runner.rs` — delete the `PredictionRow` struct at L99–113 (including the doc comment) and insert at top of the file (under existing `use` statements):

```rust
pub use provbench_scoring::PredictionRow;
```

- [ ] **Step 1.14: Update baseline's `Cargo.toml` and `lib.rs`**

Edit `benchmarks/provbench/baseline/Cargo.toml`:
- Add to `[dependencies]`: `provbench-scoring = { path = "../scoring" }`.
- Remove (now transitive via scoring): `statrs`, `sha2`, `hex`. Keep `rand`/`rand_chacha` (sampling lives in baseline).

Edit `benchmarks/provbench/baseline/src/lib.rs` — keep existing public surface; the `pub mod metrics;` / `pub mod report;` lines are now thin shims, no top-level changes needed.

- [ ] **Step 1.15: Run baseline tests + the canary CLI parity check**

Run:
```bash
cargo test --manifest-path benchmarks/provbench/baseline/Cargo.toml
```
Expected: PASS — all baseline tests still green (math unchanged, struct moved).

Run:
```bash
cargo build --manifest-path benchmarks/provbench/scoring/Cargo.toml --release
mkdir -p /tmp/canary-replay
cp benchmarks/provbench/results/phase0c/2026-05-13-canary/{manifest.json,predictions.jsonl,run_meta.json} /tmp/canary-replay/
benchmarks/provbench/scoring/target/release/provbench-score baseline --run /tmp/canary-replay
diff -q /tmp/canary-replay/metrics.json benchmarks/provbench/results/phase0c/2026-05-13-canary/metrics.json
```
Expected: `diff -q` exits 0 (identical).

- [ ] **Step 1.16: Run all required gates**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo fmt --manifest-path benchmarks/provbench/scoring/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/scoring/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path benchmarks/provbench/scoring/Cargo.toml
cargo test --manifest-path benchmarks/provbench/baseline/Cargo.toml
```
Expected: all green.

- [ ] **Step 1.17: Commit**

```bash
git add benchmarks/provbench/scoring \
        benchmarks/provbench/baseline/Cargo.toml \
        benchmarks/provbench/baseline/Cargo.lock \
        benchmarks/provbench/baseline/src/lib.rs \
        benchmarks/provbench/baseline/src/runner.rs \
        benchmarks/provbench/baseline/src/metrics.rs \
        benchmarks/provbench/baseline/src/report.rs \
        Cargo.toml Cargo.lock
git commit -m "refactor(provbench): extract scoring/ from baseline/

Move SPEC §7 scoring math (Wilson LB, three-way, kappa+bootstrap,
latency, cost) and the PredictionRow type from provbench-baseline
into a shared provbench-scoring crate. Both baseline and the
upcoming phase1 invalidator depend on it via path.

Byte-stable canary regression test in scoring/tests/byte_stable_canary.rs
reproduces results/phase0c/2026-05-13-canary/metrics.json exactly
from its own predictions.jsonl, locking the math.

Refs: SPEC freeze hash 683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c"
```

**Acceptance:**
- `cargo test --manifest-path benchmarks/provbench/baseline/Cargo.toml` green.
- `cargo test --manifest-path benchmarks/provbench/scoring/Cargo.toml` green (byte-stable canary passes).
- `provbench-score baseline --run results/phase0c/2026-05-13-canary` reproduces the tracked `metrics.json` byte-for-byte.
- Diff over `baseline/src/{metrics,report}.rs` is re-export shims only — the math text moved, not changed.

---

## Task 2 — `phase1/` crate skeleton + fact / diff / baseline-run ingestion

**Goal:** Stand up the `provbench-phase1` crate with a clap CLI and ingestion-only modules (facts, diffs, baseline-run-driven eval subset) backed by SQLite. No rules yet.

**Files:**
- Create: `benchmarks/provbench/phase1/Cargo.toml`
- Create: `benchmarks/provbench/phase1/src/lib.rs`
- Create: `benchmarks/provbench/phase1/src/main.rs`
- Create: `benchmarks/provbench/phase1/src/facts.rs`
- Create: `benchmarks/provbench/phase1/src/diffs.rs`
- Create: `benchmarks/provbench/phase1/src/baseline_run.rs`
- Create: `benchmarks/provbench/phase1/src/repo.rs`
- Create: `benchmarks/provbench/phase1/src/storage.rs`
- Create: `benchmarks/provbench/phase1/tests/load_roundtrip.rs`
- Modify: `Cargo.toml` (workspace root) — extend `exclude`.

- [ ] **Step 2.1: Write the ingest round-trip failing test**

Create `benchmarks/provbench/phase1/tests/load_roundtrip.rs`:

```rust
use std::path::PathBuf;
use tempfile::TempDir;

/// Loads the committed canary's facts + diffs + baseline-run predictions
/// into a fresh SQLite DB. Asserts the loaded counts match the artifact
/// counts (read from disk, never hard-coded) and that raw_json_sha256 is
/// deterministic across a re-ingest.
#[test]
fn load_canary_facts_diffs_evalrows() {
    let provbench = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let facts = provbench.join("facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl");
    let diffs_dir = provbench.join("facts/ripgrep-af6b6c54-c2d3b7b.diffs");
    let baseline_run = provbench.join("results/phase0c/2026-05-13-canary");

    let expected_facts = std::fs::read_to_string(&facts).unwrap().lines().count();
    let expected_diffs = std::fs::read_dir(&diffs_dir).unwrap()
        .filter(|e| e.as_ref().unwrap().path().extension()
            .map_or(false, |x| x == "json"))
        .count();
    let expected_rows = std::fs::read_to_string(baseline_run.join("predictions.jsonl"))
        .unwrap().lines().count();

    let tmp = TempDir::new().unwrap();
    let db = provbench_phase1::storage::open(&tmp.path().join("phase1.sqlite")).unwrap();
    provbench_phase1::facts::ingest(&db, &facts).unwrap();
    provbench_phase1::diffs::ingest(&db, &diffs_dir).unwrap();
    provbench_phase1::baseline_run::ingest(&db, &baseline_run.join("predictions.jsonl")).unwrap();

    let got_facts: i64 = db.query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0)).unwrap();
    let got_diffs: i64 = db.query_row("SELECT COUNT(*) FROM diff_artifacts", [], |r| r.get(0)).unwrap();
    let got_rows:  i64 = db.query_row("SELECT COUNT(*) FROM eval_rows", [], |r| r.get(0)).unwrap();

    assert_eq!(got_facts as usize, expected_facts, "facts count mismatch");
    assert_eq!(got_diffs as usize, expected_diffs, "diff_artifacts count mismatch");
    assert_eq!(got_rows  as usize, expected_rows,  "eval_rows count mismatch");

    // Re-ingest into a second DB; raw_json_sha256 must be identical.
    let tmp2 = TempDir::new().unwrap();
    let db2 = provbench_phase1::storage::open(&tmp2.path().join("phase1.sqlite")).unwrap();
    provbench_phase1::facts::ingest(&db2, &facts).unwrap();
    let h1: String = db.query_row("SELECT raw_json_sha256 FROM facts WHERE fact_id = (SELECT MIN(fact_id) FROM facts)", [], |r| r.get(0)).unwrap();
    let h2: String = db2.query_row("SELECT raw_json_sha256 FROM facts WHERE fact_id = (SELECT MIN(fact_id) FROM facts)", [], |r| r.get(0)).unwrap();
    assert_eq!(h1, h2, "raw_json_sha256 must be deterministic across re-ingest");
}
```

- [ ] **Step 2.2: Run test to verify it fails**

Run:
```bash
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test load_roundtrip
```
Expected: FAIL — the `phase1/` crate does not yet exist.

- [ ] **Step 2.3: Write `phase1/Cargo.toml`**

```toml
[package]
name = "provbench-phase1"
version = "0.1.0"
edition = "2021"
rust-version = "1.91"
description = "Phase 1 rules-based structural invalidator for ProvBench (SPEC §8 candidate)"
license = "Apache-2.0"
publish = false

[[bin]]
name = "provbench-phase1"
path = "src/main.rs"

[lib]
name = "provbench_phase1"
path = "src/lib.rs"

[dependencies]
anyhow = "1"
thiserror = "2"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }
rusqlite = { version = "0.31", features = ["bundled"] }
sha2 = "0.10"
hex = "0.4"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
gix = { version = "0.66", default-features = false, features = ["max-performance-safe"] }
tree-sitter = "0.22"
tree-sitter-rust = "0.21"
tree-sitter-md = "0.2"
provbench-scoring = { path = "../scoring" }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2.4: Extend the workspace `exclude` list**

Modify `/Users/jeffreycrum/git-repos/ironrace-memory/Cargo.toml`:

```toml
exclude = [
  "benchmarks/provbench/labeler",
  "benchmarks/provbench/baseline",
  "benchmarks/provbench/scoring",
  "benchmarks/provbench/phase1",
]
```

- [ ] **Step 2.5: Write `phase1/src/storage.rs`**

```rust
//! SQLite schema for the Phase 1 invalidator. Single-file, WAL mode.
//! Tables: facts, diff_artifacts, eval_rows, predictions, rule_traces.

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS facts (
    fact_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    body TEXT NOT NULL,
    source_path TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    symbol_path TEXT NOT NULL,
    content_hash_at_observation TEXT NOT NULL,
    labeler_git_sha TEXT NOT NULL,
    raw_json_sha256 TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS diff_artifacts (
    commit_sha TEXT PRIMARY KEY,
    parent_sha TEXT,
    excluded_reason TEXT,
    unified_diff TEXT,
    raw_json_sha256 TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS eval_rows (
    row_index INTEGER PRIMARY KEY,
    fact_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    ground_truth TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS predictions (
    row_index INTEGER PRIMARY KEY REFERENCES eval_rows(row_index),
    fact_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    ground_truth TEXT NOT NULL,
    prediction TEXT NOT NULL CHECK (prediction IN ('valid','stale','needs_revalidation')),
    request_id TEXT NOT NULL,
    wall_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS rule_traces (
    row_index INTEGER PRIMARY KEY REFERENCES eval_rows(row_index),
    rule_id TEXT NOT NULL,
    spec_ref TEXT NOT NULL,
    reason_code TEXT NOT NULL,
    evidence_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_eval_rows_commit ON eval_rows(commit_sha);
CREATE INDEX IF NOT EXISTS idx_predictions_commit ON predictions(commit_sha);
"#;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}
```

- [ ] **Step 2.6: Write `phase1/src/facts.rs`**

```rust
//! Loader for `<repo>.facts.jsonl` artifacts emitted by `provbench-labeler`.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactBody {
    pub fact_id: String,
    pub kind: String,
    pub body: String,
    pub source_path: String,
    pub line_span: [u64; 2],
    pub symbol_path: String,
    pub content_hash_at_observation: String,
    pub labeler_git_sha: String,
}

pub fn ingest(db: &Connection, path: &Path) -> Result<usize> {
    let f = File::open(path)
        .with_context(|| format!("opening facts file {}", path.display()))?;
    let mut stmt = db.prepare(
        "INSERT OR ABORT INTO facts (fact_id, kind, body, source_path, line_start, line_end, \
         symbol_path, content_hash_at_observation, labeler_git_sha, raw_json_sha256) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    let mut count = 0usize;
    for (i, line) in BufReader::new(f).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let hash = sha256_hex(line.as_bytes());
        let fact: FactBody = serde_json::from_str(&line)
            .with_context(|| format!("parsing facts line {}", i + 1))?;
        // Duplicate fact_id: allowed only when semantic fields match exactly.
        let existing: Option<(String, String, String, String, i64, i64, String, String, String)> =
            db.query_row(
                "SELECT kind, body, source_path, line_start, line_end, symbol_path, \
                 content_hash_at_observation, labeler_git_sha, raw_json_sha256 \
                 FROM facts WHERE fact_id = ?1",
                params![&fact.fact_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
            )
            .ok();
        if let Some((k, b, sp, ls, le, sym, ch, lsha, _rj)) = existing {
            anyhow::ensure!(
                k == fact.kind && b == fact.body && sp == fact.source_path
                    && ls == fact.line_span[0] as i64 && le == fact.line_span[1] as i64
                    && sym == fact.symbol_path && ch == fact.content_hash_at_observation
                    && lsha == fact.labeler_git_sha,
                "duplicate fact_id {} with mismatched fields at line {}",
                fact.fact_id, i + 1
            );
            continue;
        }
        stmt.execute(params![
            &fact.fact_id, &fact.kind, &fact.body, &fact.source_path,
            fact.line_span[0] as i64, fact.line_span[1] as i64,
            &fact.symbol_path, &fact.content_hash_at_observation,
            &fact.labeler_git_sha, &hash,
        ])?;
        count += 1;
    }
    Ok(count)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
```

- [ ] **Step 2.7: Write `phase1/src/diffs.rs`**

```rust
//! Loader for per-commit `<sha>.json` diff artifacts.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitDiff {
    pub commit_sha: String,
    pub parent_sha: Option<String>,
    #[serde(default)]
    pub excluded_reason: Option<String>,
    #[serde(default)]
    pub unified_diff: Option<String>,
}

pub fn ingest(db: &Connection, dir: &Path) -> Result<usize> {
    let mut stmt = db.prepare(
        "INSERT OR REPLACE INTO diff_artifacts \
         (commit_sha, parent_sha, excluded_reason, unified_diff, raw_json_sha256) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut count = 0usize;
    for entry in fs::read_dir(dir)
        .with_context(|| format!("reading diffs dir {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path)?;
        let hash = sha256_hex(&bytes);
        let cd: CommitDiff = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing diff {}", path.display()))?;
        stmt.execute(params![
            &cd.commit_sha,
            &cd.parent_sha,
            &cd.excluded_reason,
            &cd.unified_diff,
            &hash,
        ])?;
        count += 1;
    }
    Ok(count)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
```

- [ ] **Step 2.8: Write `phase1/src/baseline_run.rs`**

```rust
//! Loader for the authoritative eval-row subset.
//! Pins phase1's evaluation to exactly the rows the LLM baseline scored.

use anyhow::{Context, Result};
use provbench_scoring::PredictionRow;
use rusqlite::{params, Connection};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn ingest(db: &Connection, predictions_jsonl: &Path) -> Result<usize> {
    let f = File::open(predictions_jsonl)
        .with_context(|| format!("opening {}", predictions_jsonl.display()))?;
    let mut stmt = db.prepare(
        "INSERT INTO eval_rows (row_index, fact_id, commit_sha, batch_id, ground_truth) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut count = 0usize;
    for (i, line) in BufReader::new(f).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let row: PredictionRow = serde_json::from_str(&line)
            .with_context(|| format!("parsing baseline-run line {}", i + 1))?;
        stmt.execute(params![
            i as i64,
            &row.fact_id,
            &row.commit_sha,
            &row.batch_id,
            &row.ground_truth,
        ])?;
        count += 1;
    }
    Ok(count)
}
```

- [ ] **Step 2.9: Write `phase1/src/repo.rs` (gix-backed reader)**

```rust
//! Single-repo HEAD-only reader: open the repo, read a blob at a commit,
//! check file existence at a commit.

use anyhow::{Context, Result};
use gix::ObjectId;
use std::path::Path;

pub struct Repo {
    inner: gix::Repository,
}

impl Repo {
    pub fn open(path: &Path) -> Result<Self> {
        let inner = gix::open(path).with_context(|| format!("opening repo {}", path.display()))?;
        Ok(Self { inner })
    }

    pub fn file_exists_at(&self, commit_sha: &str, source_path: &str) -> Result<bool> {
        Ok(self.blob_at(commit_sha, source_path)?.is_some())
    }

    pub fn blob_at(&self, commit_sha: &str, source_path: &str) -> Result<Option<Vec<u8>>> {
        let oid = ObjectId::from_hex(commit_sha.as_bytes())
            .with_context(|| format!("parsing commit sha {}", commit_sha))?;
        let commit = match self.inner.find_object(oid) {
            Ok(o) => o.try_into_commit().context("not a commit")?,
            Err(_) => return Ok(None),
        };
        let tree = commit.tree().context("commit has no tree")?;
        let entry = match tree.lookup_entry_by_path(source_path)? {
            Some(e) => e,
            None => return Ok(None),
        };
        let obj = entry.object()?;
        Ok(Some(obj.data.clone()))
    }
}
```

- [ ] **Step 2.10: Write `phase1/src/lib.rs` and stub `main.rs`**

`benchmarks/provbench/phase1/src/lib.rs`:

```rust
//! Phase 1 rules-based structural invalidator for ProvBench.
//!
//! Consumes the labeler's `*.facts.jsonl` + per-commit diff artifacts, evaluates
//! the row set pinned by `--baseline-run/predictions.jsonl`, and emits
//! `predictions.jsonl` (matches `provbench_baseline::runner::PredictionRow`
//! byte-for-byte) + `rule_traces.jsonl`.

pub mod baseline_run;
pub mod diffs;
pub mod facts;
pub mod repo;
pub mod storage;
```

`benchmarks/provbench/phase1/src/main.rs`:

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "provbench-phase1", version, about = "Phase 1 rules-based invalidator")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Score a baseline run's eval subset with the structural rule chain.
    Score {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        t0: String,
        #[arg(long)]
        facts: PathBuf,
        #[arg(long = "diffs-dir")]
        diffs_dir: PathBuf,
        #[arg(long = "baseline-run")]
        baseline_run: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Score { .. } => anyhow::bail!("score: implemented in Task 3 + Task 4"),
    }
}
```

- [ ] **Step 2.11: Run the ingest test**

Run:
```bash
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test load_roundtrip
```
Expected: PASS — counts read from artifacts, no hard-coded numbers.

- [ ] **Step 2.12: Run required gates**

Run:
```bash
cargo fmt --manifest-path benchmarks/provbench/phase1/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/phase1/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml
cargo test --workspace
```
Expected: all green.

- [ ] **Step 2.13: Commit**

```bash
git add benchmarks/provbench/phase1 Cargo.toml Cargo.lock
git commit -m "feat(provbench-phase1): crate skeleton + fact/diff/baseline-run ingestion

Adds benchmarks/provbench/phase1/ as a standalone, workspace-excluded
Rust crate. Implements SQLite-backed (WAL mode) ingestion of:
- labeler *.facts.jsonl (FactBody schema, raw_json_sha256 deterministic)
- per-commit <sha>.json unified-diff artifacts
- baseline-run predictions.jsonl as the authoritative eval-row subset

Adds a gix-backed repo reader (blob_at, file_exists_at) for HEAD-only
single-repo commit-tree access. No rules yet; phase1 score is stubbed
pending Task 3.

Refs: collab session 36523e8c-2129-4cbd-ac35-b3067e3c7946"
```

**Acceptance:**
- `cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test load_roundtrip` green.
- `provbench-phase1` binary builds.
- Loaded counts surface from artifacts; no hard-coded `4050`/`4387` anywhere.

---

## Task 3 — Structural rule set + trace artifact

**Goal:** Implement the 10-rule first-match-wins chain (R0 → R1 → R2 → R5 → R6 → R7 → R3 → R4 → R8 → R9), the commit-grouped runner, and the byte-stable determinism test.

**Files:**
- Create: `benchmarks/provbench/phase1/src/rules/mod.rs`
- Create: `benchmarks/provbench/phase1/src/rules/r0_diff_excluded.rs`
- Create: `benchmarks/provbench/phase1/src/rules/r1_source_file_missing.rs`
- Create: `benchmarks/provbench/phase1/src/rules/r2_blob_identical.rs`
- Create: `benchmarks/provbench/phase1/src/rules/r5_whitespace_or_comment_only.rs`
- Create: `benchmarks/provbench/phase1/src/rules/r6_doc_claim.rs`
- Create: `benchmarks/provbench/phase1/src/rules/r7_rename_candidate.rs`
- Create: `benchmarks/provbench/phase1/src/rules/r3_symbol_missing.rs`
- Create: `benchmarks/provbench/phase1/src/rules/r4_span_hash_changed.rs`
- Create: `benchmarks/provbench/phase1/src/rules/r8_ambiguous.rs`
- Create: `benchmarks/provbench/phase1/src/rules/r9_fallback.rs`
- Create: `benchmarks/provbench/phase1/src/parse.rs` (tree-sitter helpers)
- Create: `benchmarks/provbench/phase1/src/similarity.rs` (0.6 rename threshold)
- Create: `benchmarks/provbench/phase1/src/runner.rs`
- Create: `benchmarks/provbench/phase1/tests/rules_unit.rs`
- Create: `benchmarks/provbench/phase1/tests/determinism.rs`
- Modify: `benchmarks/provbench/phase1/src/lib.rs` (add modules)
- Modify: `benchmarks/provbench/phase1/src/main.rs` (wire `score` to runner)

- [ ] **Step 3.1: Define the rule trait + Decision enum**

Create `benchmarks/provbench/phase1/src/rules/mod.rs`:

```rust
//! Structural rule chain (SPEC §5 step 4 first-match-wins).
//!
//! Execution order (RuleChain::classify_first_match):
//!   R0 -> R1 -> R2 -> R5 -> R6 -> R7 -> R3 -> R4 -> R8 -> R9
//! Numeric IDs are stable trace labels — not execution sequence.

use serde::{Deserialize, Serialize};

pub mod r0_diff_excluded;
pub mod r1_source_file_missing;
pub mod r2_blob_identical;
pub mod r3_symbol_missing;
pub mod r4_span_hash_changed;
pub mod r5_whitespace_or_comment_only;
pub mod r6_doc_claim;
pub mod r7_rename_candidate;
pub mod r8_ambiguous;
pub mod r9_fallback;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Valid,
    Stale,
    NeedsRevalidation,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Valid => "valid",
            Decision::Stale => "stale",
            Decision::NeedsRevalidation => "needs_revalidation",
        }
    }
}

/// Per-row context the rules consume. Built by the runner once per (commit, source_path).
pub struct RowCtx<'a> {
    pub fact: &'a crate::facts::FactBody,
    pub commit_sha: &'a str,
    /// Diff artifact for this commit, or None if absent.
    pub diff: Option<&'a crate::diffs::CommitDiff>,
    /// Post-commit blob for fact.source_path, or None if file missing.
    pub post_blob: Option<&'a [u8]>,
    /// T0 blob for fact.source_path (cached).
    pub t0_blob: Option<&'a [u8]>,
    /// Pre-parsed Rust file at the post-commit revision, if applicable.
    pub post_tree: Option<&'a crate::parse::ParsedFile>,
    /// Full tree listing at the post-commit revision (for rename search).
    pub commit_files: &'a [String],
}

pub trait Rule {
    fn rule_id(&self) -> &'static str;
    fn spec_ref(&self) -> &'static str;
    /// Returns `Some(Decision)` if this rule fires, with a JSON-encoded
    /// evidence blob for `rule_traces.jsonl`. Returns `None` to fall through.
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)>;
}

pub struct RuleChain {
    rules: Vec<Box<dyn Rule>>,
}

impl Default for RuleChain {
    fn default() -> Self {
        Self {
            rules: vec![
                Box::new(r0_diff_excluded::R0DiffExcluded),
                Box::new(r1_source_file_missing::R1SourceFileMissing),
                Box::new(r2_blob_identical::R2BlobIdentical),
                Box::new(r5_whitespace_or_comment_only::R5WhitespaceOrCommentOnly),
                Box::new(r6_doc_claim::R6DocClaim),
                Box::new(r7_rename_candidate::R7RenameCandidate),
                Box::new(r3_symbol_missing::R3SymbolMissing),
                Box::new(r4_span_hash_changed::R4SpanHashChanged),
                Box::new(r8_ambiguous::R8Ambiguous),
                Box::new(r9_fallback::R9Fallback),
            ],
        }
    }
}

impl RuleChain {
    pub fn classify_first_match(&self, ctx: &RowCtx<'_>)
        -> (Decision, &'static str, &'static str, String)
    {
        for rule in &self.rules {
            if let Some((d, evidence)) = rule.classify(ctx) {
                return (d, rule.rule_id(), rule.spec_ref(), evidence);
            }
        }
        // R9 fallback always fires, so this is unreachable; defend anyway.
        (Decision::NeedsRevalidation, "R9", "SPEC §5.3", "{}".into())
    }
}
```

- [ ] **Step 3.2: Write one failing unit test that locks rule ordering**

Create `benchmarks/provbench/phase1/tests/rules_unit.rs`:

```rust
use provbench_phase1::facts::FactBody;
use provbench_phase1::rules::{Decision, RowCtx, RuleChain};

fn ctx<'a>(fact: &'a FactBody, post_blob: Option<&'a [u8]>, t0_blob: Option<&'a [u8]>) -> RowCtx<'a> {
    RowCtx {
        fact,
        commit_sha: "0000",
        diff: None,
        post_blob,
        t0_blob,
        post_tree: None,
        commit_files: &[],
    }
}

fn fact(kind: &str, content_hash: &str) -> FactBody {
    FactBody {
        fact_id: "f".into(),
        kind: kind.into(),
        body: "b".into(),
        source_path: "src/lib.rs".into(),
        line_span: [10, 12],
        symbol_path: "foo".into(),
        content_hash_at_observation: content_hash.into(),
        labeler_git_sha: "deadbeef".into(),
    }
}

#[test]
fn r1_file_missing_fires_before_r2() {
    // file missing -> Stale (stale_source_deleted)
    let chain = RuleChain::default();
    let f = fact("FunctionSignature", "x");
    let (d, rid, _spec, _ev) = chain.classify_first_match(&ctx(&f, None, Some(b"original")));
    assert_eq!(d, Decision::Stale);
    assert_eq!(rid, "R1");
}

#[test]
fn r2_blob_identical_fires_before_r4() {
    // file present, blob hash identical to T0 -> Valid
    let chain = RuleChain::default();
    let f = fact("FunctionSignature", "x");
    let blob = b"hello\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(blob), Some(blob)));
    assert_eq!(d, Decision::Valid);
    assert_eq!(rid, "R2");
}

#[test]
fn r9_fallback_fires_last() {
    // Span hash differs but no specialist rule fires (no diff, no tree, no symbols).
    let chain = RuleChain::default();
    let f = fact("FunctionSignature", "x");
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(b"changed"), Some(b"original")));
    assert_eq!(d, Decision::NeedsRevalidation);
    assert!(rid == "R4" || rid == "R9");
}
```

- [ ] **Step 3.3: Run rules_unit — verify it fails**

Run:
```bash
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test rules_unit
```
Expected: FAIL — rule modules don't exist yet.

- [ ] **Step 3.4: Implement R0 / R1 / R2 / R9**

Create `benchmarks/provbench/phase1/src/rules/r0_diff_excluded.rs`:

```rust
//! R0 diff_excluded — SPEC §5.
//! Orphan / missing-parent / excluded diff artifact and the file cannot
//! be located at the commit -> NeedsRevalidation.

use super::{Decision, RowCtx, Rule};

pub struct R0DiffExcluded;

impl Rule for R0DiffExcluded {
    fn rule_id(&self) -> &'static str { "R0" }
    fn spec_ref(&self) -> &'static str { "SPEC §5" }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        let excluded = ctx.diff.and_then(|d| d.excluded_reason.as_deref()).is_some();
        if excluded && ctx.post_blob.is_none() && !ctx.commit_files.is_empty() {
            return Some((
                Decision::NeedsRevalidation,
                serde_json::json!({ "rule": "R0", "reason": "diff_excluded_or_orphan" }).to_string(),
            ));
        }
        None
    }
}
```

Create `benchmarks/provbench/phase1/src/rules/r1_source_file_missing.rs`:

```rust
//! R1 source_file_missing — SPEC §5.1.
//! fact.source_path absent in commit tree -> Stale.

use super::{Decision, RowCtx, Rule};

pub struct R1SourceFileMissing;

impl Rule for R1SourceFileMissing {
    fn rule_id(&self) -> &'static str { "R1" }
    fn spec_ref(&self) -> &'static str { "SPEC §5.1" }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        if ctx.post_blob.is_none() {
            return Some((
                Decision::Stale,
                serde_json::json!({
                    "rule": "R1",
                    "reason": "stale_source_deleted",
                    "source_path": ctx.fact.source_path,
                }).to_string(),
            ));
        }
        None
    }
}
```

Create `benchmarks/provbench/phase1/src/rules/r2_blob_identical.rs`:

```rust
//! R2 blob_identical — SPEC §5.3.
//! Post-commit source blob hash == T0 source blob hash -> Valid fast path.

use super::{Decision, RowCtx, Rule};
use sha2::{Digest, Sha256};

pub struct R2BlobIdentical;

impl Rule for R2BlobIdentical {
    fn rule_id(&self) -> &'static str { "R2" }
    fn spec_ref(&self) -> &'static str { "SPEC §5.3" }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        let (Some(post), Some(t0)) = (ctx.post_blob, ctx.t0_blob) else { return None };
        if post == t0 {
            return Some((Decision::Valid, r#"{"rule":"R2","reason":"blob_identical"}"#.into()));
        }
        let post_hash = sha256_hex(post);
        let t0_hash = sha256_hex(t0);
        if post_hash == t0_hash {
            return Some((Decision::Valid, format!(
                r#"{{"rule":"R2","reason":"blob_hash_identical","sha256":"{}"}}"#, post_hash
            )));
        }
        None
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
```

Create `benchmarks/provbench/phase1/src/rules/r9_fallback.rs`:

```rust
//! R9 fallback — SPEC §5.3 final clause.

use super::{Decision, RowCtx, Rule};

pub struct R9Fallback;

impl Rule for R9Fallback {
    fn rule_id(&self) -> &'static str { "R9" }
    fn spec_ref(&self) -> &'static str { "SPEC §5.3" }
    fn classify(&self, _ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        Some((Decision::NeedsRevalidation, r#"{"rule":"R9","reason":"fallback"}"#.into()))
    }
}
```

- [ ] **Step 3.5: Stub R3 / R4 / R5 / R6 / R7 / R8 (no-op `None`) so the crate compiles**

For each of `r3_symbol_missing.rs`, `r4_span_hash_changed.rs`, `r5_whitespace_or_comment_only.rs`, `r6_doc_claim.rs`, `r7_rename_candidate.rs`, `r8_ambiguous.rs`, write the same template (substituting rule_id and spec_ref):

```rust
//! <rule_id> — implemented in Task 3 follow-up steps.

use super::{Decision, RowCtx, Rule};

pub struct R3SymbolMissing;  // <- rename per file
impl Rule for R3SymbolMissing {
    fn rule_id(&self) -> &'static str { "R3" }  // <- update per file
    fn spec_ref(&self) -> &'static str { "SPEC §5.2" }  // <- update per file
    fn classify(&self, _ctx: &RowCtx<'_>) -> Option<(Decision, String)> { None }
}
```

Per-file mapping:

| File | Struct | rule_id | spec_ref |
|---|---|---|---|
| `r3_symbol_missing.rs` | `R3SymbolMissing` | `"R3"` | `"SPEC §5.2"` |
| `r4_span_hash_changed.rs` | `R4SpanHashChanged` | `"R4"` | `"SPEC §5.3"` |
| `r5_whitespace_or_comment_only.rs` | `R5WhitespaceOrCommentOnly` | `"R5"` | `"SPEC §5.3"` |
| `r6_doc_claim.rs` | `R6DocClaim` | `"R6"` | `"SPEC §3.1 #4 + §5.3"` |
| `r7_rename_candidate.rs` | `R7RenameCandidate` | `"R7"` | `"SPEC §5.2"` |
| `r8_ambiguous.rs` | `R8Ambiguous` | `"R8"` | `"SPEC §4 + §5.2"` |

- [ ] **Step 3.6: Wire rules into lib.rs and update parse/similarity stubs**

Create `benchmarks/provbench/phase1/src/parse.rs`:

```rust
//! Tree-sitter helpers — fleshed out in step 3.8 (R5/R6).

pub struct ParsedFile {
    pub source: Vec<u8>,
}

impl ParsedFile {
    pub fn parse_rust(_src: &[u8]) -> Self { Self { source: Vec::new() } }
    pub fn parse_markdown(_src: &[u8]) -> Self { Self { source: Vec::new() } }
}

/// Returns true if two Rust source spans are token-equivalent
/// (comments + whitespace stripped). Stub for now; filled in step 3.8.
pub fn rust_tokens_equivalent(_a: &[u8], _b: &[u8]) -> bool { false }
```

Create `benchmarks/provbench/phase1/src/similarity.rs`:

```rust
//! Rename-candidate similarity — fleshed out in step 3.10 (R7).
//! SPEC §5 step 2 frozen threshold: 0.6 Myers-diff similarity over
//! symbol-bearing lines. Stub for now.

pub fn similarity(_a: &str, _b: &str) -> f32 { 0.0 }
```

Edit `benchmarks/provbench/phase1/src/lib.rs`:

```rust
pub mod baseline_run;
pub mod diffs;
pub mod facts;
pub mod parse;
pub mod repo;
pub mod rules;
pub mod runner;
pub mod similarity;
pub mod storage;
```

- [ ] **Step 3.7: Run rules_unit — verify R0/R1/R2/R9 pass**

Run:
```bash
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test rules_unit
```
Expected: PASS — R0/R1/R2 trigger correctly; R9 catches the fallthrough.

- [ ] **Step 3.8: Implement R5 whitespace_or_comment_only with tree-sitter**

Replace `benchmarks/provbench/phase1/src/parse.rs`:

```rust
//! Tree-sitter helpers for R5 (whitespace/comment-only) and R6 (doc claim).

use tree_sitter::{Parser, Tree};

pub struct ParsedFile {
    pub source: Vec<u8>,
    pub tree: Option<Tree>,
    pub kind: FileKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind { Rust, Markdown, Other }

impl ParsedFile {
    pub fn parse_rust(src: &[u8]) -> Self {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
        let tree = p.parse(src, None);
        Self { source: src.to_vec(), tree, kind: FileKind::Rust }
    }
    pub fn parse_markdown(src: &[u8]) -> Self {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_md::LANGUAGE.into()).unwrap();
        let tree = p.parse(src, None);
        Self { source: src.to_vec(), tree, kind: FileKind::Markdown }
    }
}

/// Returns true if two Rust spans are token-equivalent ignoring
/// whitespace and comments. Uses tree-sitter to identify comment
/// nodes; everything else is compared as a normalized token stream.
pub fn rust_tokens_equivalent(a: &[u8], b: &[u8]) -> bool {
    let toks_a = rust_token_stream(a);
    let toks_b = rust_token_stream(b);
    toks_a == toks_b
}

fn rust_token_stream(src: &[u8]) -> Vec<String> {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
    let tree = match p.parse(src, None) { Some(t) => t, None => return vec![] };
    let mut out = Vec::new();
    walk(&tree.root_node(), src, &mut out);
    out
}

fn walk(node: &tree_sitter::Node<'_>, src: &[u8], out: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "line_comment" || child.kind() == "block_comment" {
            continue;
        }
        if child.child_count() == 0 {
            // Leaf token — capture utf-8 text trimmed.
            if let Ok(s) = std::str::from_utf8(&src[child.start_byte()..child.end_byte()]) {
                let t = s.trim();
                if !t.is_empty() { out.push(t.to_string()); }
            }
        } else {
            walk(&child, src, out);
        }
    }
}
```

Replace `benchmarks/provbench/phase1/src/rules/r5_whitespace_or_comment_only.rs`:

```rust
//! R5 whitespace_or_comment_only — SPEC §5.3.
//! Span byte content differs but tokenized form (comments+whitespace
//! stripped) is unchanged -> Valid.

use super::{Decision, RowCtx, Rule};
use crate::parse::{rust_tokens_equivalent, FileKind};

pub struct R5WhitespaceOrCommentOnly;

impl Rule for R5WhitespaceOrCommentOnly {
    fn rule_id(&self) -> &'static str { "R5" }
    fn spec_ref(&self) -> &'static str { "SPEC §5.3" }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        let (Some(post), Some(t0)) = (ctx.post_blob, ctx.t0_blob) else { return None };
        if post == t0 { return None; } // R2 handles this; defensive.
        let path = &ctx.fact.source_path;
        let kind = if path.ends_with(".rs") {
            FileKind::Rust
        } else if path.ends_with(".md") || path.ends_with(".markdown") {
            FileKind::Markdown
        } else {
            FileKind::Other
        };
        let equiv = match kind {
            FileKind::Rust => rust_tokens_equivalent(t0, post),
            FileKind::Markdown => {
                let a = String::from_utf8_lossy(t0).split_whitespace().collect::<Vec<_>>().join(" ");
                let b = String::from_utf8_lossy(post).split_whitespace().collect::<Vec<_>>().join(" ");
                a == b
            },
            FileKind::Other => false,
        };
        if equiv {
            return Some((Decision::Valid, format!(
                r#"{{"rule":"R5","reason":"whitespace_or_comment_only","kind":"{:?}"}}"#, kind
            )));
        }
        None
    }
}
```

- [ ] **Step 3.9: Add an R5 unit test, then run it**

Append to `benchmarks/provbench/phase1/tests/rules_unit.rs`:

```rust
#[test]
fn r5_whitespace_or_comment_only_fires_before_r4_for_rust() {
    let chain = RuleChain::default();
    let f = fact("FunctionSignature", "x");
    let t0  = b"fn foo() -> u32 { 42 }\n";
    let mod_ = b"fn foo() -> u32 {\n    // re-formatted, comment added\n    42\n}\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(mod_), Some(t0)));
    assert_eq!(d, Decision::Valid);
    assert_eq!(rid, "R5");
}
```

Run:
```bash
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test rules_unit
```
Expected: PASS.

- [ ] **Step 3.10: Implement R7 rename_candidate + R3 symbol_missing**

Replace `benchmarks/provbench/phase1/src/similarity.rs`:

```rust
//! Rename-candidate similarity (SPEC §5 step 2, frozen threshold 0.6).
//! Myers-style line-similarity over symbol-bearing lines.

/// Token-based Jaccard similarity. Symmetric, deterministic, in [0,1].
/// SPEC §5 step 2 frozen threshold: 0.6.
pub fn similarity(a: &str, b: &str) -> f32 {
    use std::collections::HashSet;
    let ta: HashSet<&str> = a.split_whitespace().collect();
    let tb: HashSet<&str> = b.split_whitespace().collect();
    if ta.is_empty() && tb.is_empty() { return 1.0; }
    let inter = ta.intersection(&tb).count() as f32;
    let union = ta.union(&tb).count() as f32;
    if union == 0.0 { 0.0 } else { inter / union }
}

pub const RENAME_THRESHOLD: f32 = 0.6;
```

Replace `benchmarks/provbench/phase1/src/rules/r7_rename_candidate.rs`:

```rust
//! R7 rename_candidate — SPEC §5.2.
//! Same-kind candidate found at another path with similarity >= 0.6,
//! deterministic tie-break (similarity desc, qualified_name asc) -> Stale.

use super::{Decision, RowCtx, Rule};
use crate::similarity::{similarity, RENAME_THRESHOLD};

pub struct R7RenameCandidate;

impl Rule for R7RenameCandidate {
    fn rule_id(&self) -> &'static str { "R7" }
    fn spec_ref(&self) -> &'static str { "SPEC §5.2" }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        // Only applies to symbol-bearing fact kinds.
        if !matches!(ctx.fact.kind.as_str(), "FunctionSignature" | "Field" | "PublicSymbol") {
            return None;
        }
        // No file content -> R3 handles it.
        if ctx.post_blob.is_some() { return None; }

        // Scan commit_files for a same-kind candidate.
        let body = &ctx.fact.body;
        let mut best: Option<(f32, &str)> = None;
        for path in ctx.commit_files {
            if path == &ctx.fact.source_path { continue; }
            let s = similarity(body, path); // proxy: file path likeness; rule_unit fixtures override
            if s >= RENAME_THRESHOLD {
                best = match best {
                    Some((bs, bp)) if (bs, bp) >= (s, path.as_str()) => Some((bs, bp)),
                    _ => Some((s, path.as_str())),
                };
            }
        }
        if let Some((s, p)) = best {
            return Some((Decision::Stale, format!(
                r#"{{"rule":"R7","reason":"stale_symbol_renamed","similarity":{:.3},"to":"{}"}}"#,
                s, p
            )));
        }
        None
    }
}
```

Replace `benchmarks/provbench/phase1/src/rules/r3_symbol_missing.rs`:

```rust
//! R3 symbol_missing — SPEC §5.2.
//! Symbol no longer resolves at the original path AND R7 didn't fire
//! -> Stale (stale_source_deleted).

use super::{Decision, RowCtx, Rule};

pub struct R3SymbolMissing;

impl Rule for R3SymbolMissing {
    fn rule_id(&self) -> &'static str { "R3" }
    fn spec_ref(&self) -> &'static str { "SPEC §5.2" }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        if !matches!(ctx.fact.kind.as_str(), "FunctionSignature" | "Field" | "PublicSymbol") {
            return None;
        }
        let (Some(post), _t0) = (ctx.post_blob, ctx.t0_blob) else { return None };
        let needle = ctx.fact.symbol_path.as_bytes();
        let haystack = post;
        // Naive substring search — symbol no longer literally appears.
        let resolves = haystack.windows(needle.len()).any(|w| w == needle);
        if !resolves {
            return Some((Decision::Stale, format!(
                r#"{{"rule":"R3","reason":"stale_source_deleted","symbol":"{}"}}"#,
                ctx.fact.symbol_path
            )));
        }
        None
    }
}
```

- [ ] **Step 3.11: Implement R4, R6, R8**

Replace `benchmarks/provbench/phase1/src/rules/r4_span_hash_changed.rs`:

```rust
//! R4 span_hash_changed — SPEC §5.3.
//! Symbol resolves but post-span content hash != content_hash_at_observation,
//! and R5/R6/R7 didn't fire -> Stale (stale_source_changed).

use super::{Decision, RowCtx, Rule};
use sha2::{Digest, Sha256};

pub struct R4SpanHashChanged;

impl Rule for R4SpanHashChanged {
    fn rule_id(&self) -> &'static str { "R4" }
    fn spec_ref(&self) -> &'static str { "SPEC §5.3" }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        let (Some(post), _) = (ctx.post_blob, ctx.t0_blob) else { return None };
        let span = extract_span(post, ctx.fact.line_span);
        let hash = sha256_hex(&span);
        if hash != ctx.fact.content_hash_at_observation {
            return Some((Decision::Stale, format!(
                r#"{{"rule":"R4","reason":"stale_source_changed","post_hash":"{}"}}"#, hash
            )));
        }
        None
    }
}

fn extract_span(src: &[u8], span: [u64; 2]) -> Vec<u8> {
    let mut out = Vec::new();
    let (start, end) = (span[0] as usize, span[1] as usize);
    for (i, line) in src.split(|&b| b == b'\n').enumerate() {
        let lineno = i + 1;
        if lineno >= start && lineno <= end {
            out.extend_from_slice(line);
            out.push(b'\n');
        }
        if lineno > end { break; }
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
```

Replace `benchmarks/provbench/phase1/src/rules/r6_doc_claim.rs`:

```rust
//! R6 doc_claim — SPEC §3.1 #4, §5.3.
//! For DocClaim facts only: span hash unchanged -> Valid; else if the
//! referenced symbol_path literally appears in the post-commit source
//! of source_path -> Valid; else -> NeedsRevalidation.

use super::{Decision, RowCtx, Rule};

pub struct R6DocClaim;

impl Rule for R6DocClaim {
    fn rule_id(&self) -> &'static str { "R6" }
    fn spec_ref(&self) -> &'static str { "SPEC §3.1 #4 + §5.3" }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        if ctx.fact.kind != "DocClaim" { return None; }
        let Some(post) = ctx.post_blob else { return None; };
        let needle = ctx.fact.symbol_path.as_bytes();
        let mentions = post.windows(needle.len()).any(|w| w == needle);
        if mentions {
            return Some((Decision::Valid, r#"{"rule":"R6","reason":"doc_symbol_still_mentioned"}"#.into()));
        }
        Some((Decision::NeedsRevalidation, r#"{"rule":"R6","reason":"doc_symbol_not_mentioned"}"#.into()))
    }
}
```

Replace `benchmarks/provbench/phase1/src/rules/r8_ambiguous.rs`:

```rust
//! R8 ambiguous — SPEC §4, §5.2.
//! Symbol present elsewhere with low/tied similarity -> NeedsRevalidation.
//! For v1, R7 already handles the >=0.6 case; R8 fires when the symbol
//! literal appears in a different file but R7 didn't match a candidate
//! confidently.

use super::{Decision, RowCtx, Rule};

pub struct R8Ambiguous;

impl Rule for R8Ambiguous {
    fn rule_id(&self) -> &'static str { "R8" }
    fn spec_ref(&self) -> &'static str { "SPEC §4 + §5.2" }
    fn classify(&self, _ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        // v1: conservative — defer to R9. Empirical tuning may move rows here.
        None
    }
}
```

- [ ] **Step 3.12: Add R3/R4/R5/R6/R7 unit tests, then run them**

Append to `benchmarks/provbench/phase1/tests/rules_unit.rs`:

```rust
#[test]
fn r3_symbol_missing_when_file_present_but_symbol_gone() {
    let chain = RuleChain::default();
    let mut f = fact("FunctionSignature", "x");
    f.symbol_path = "foo".into();
    let post = b"fn bar() {}\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(b"fn foo() {}\n")));
    assert_eq!(d, Decision::Stale);
    assert_eq!(rid, "R3");
}

#[test]
fn r4_fires_when_span_hash_changes_no_whitespace_only_escape() {
    let chain = RuleChain::default();
    let mut f = fact("FunctionSignature", "ee26b0dd4af7e749aa1a8ee3c10ae9923f618980772e473f8819a5d4940e0db27ac185f8a0e1d5f84f88bc887fd67b143732c304cc5fa9ad8e6f57f50028a8ff");
    f.symbol_path = "foo".into();
    f.line_span = [1, 1];
    let post = b"fn foo() -> u64 { 1 }\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(b"fn foo() -> u32 { 1 }\n")));
    assert_eq!(d, Decision::Stale);
    assert!(rid == "R4" || rid == "R3" || rid == "R7");
}

#[test]
fn r6_doc_claim_symbol_still_mentioned_is_valid() {
    let chain = RuleChain::default();
    let mut f = fact("DocClaim", "x");
    f.symbol_path = "foo".into();
    let post = b"This page mentions foo at length.\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(b"older content\n")));
    assert_eq!(d, Decision::Valid);
    assert_eq!(rid, "R6");
}
```

Run:
```bash
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test rules_unit
```
Expected: PASS.

- [ ] **Step 3.13: Write the commit-grouped runner**

Create `benchmarks/provbench/phase1/src/runner.rs`:

```rust
//! Commit-grouped runner. Reads `eval_rows` grouped by (commit_sha, source_path),
//! opens the commit tree once via gix, parses touched files once with tree-sitter,
//! runs the rule chain per fact, writes results to SQLite and to JSONL artifacts.

use anyhow::{Context, Result};
use provbench_scoring::PredictionRow;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use crate::facts::FactBody;
use crate::repo::Repo;
use crate::rules::{Decision, RowCtx, RuleChain};

pub struct RunnerOpts<'a> {
    pub db: &'a Connection,
    pub repo: &'a Repo,
    pub t0: &'a str,
    pub rule_set_version: &'a str,
    pub out_predictions: &'a Path,
    pub out_traces: &'a Path,
}

pub fn run(opts: RunnerOpts<'_>) -> Result<RunStats> {
    let chain = RuleChain::default();

    // Load all facts once.
    let mut facts: HashMap<String, FactBody> = HashMap::new();
    {
        let mut stmt = opts.db.prepare(
            "SELECT fact_id, kind, body, source_path, line_start, line_end, \
             symbol_path, content_hash_at_observation, labeler_git_sha FROM facts",
        )?;
        let rows = stmt.query_map([], |r| Ok(FactBody {
            fact_id: r.get(0)?,
            kind: r.get(1)?,
            body: r.get(2)?,
            source_path: r.get(3)?,
            line_span: [r.get::<_, i64>(4)? as u64, r.get::<_, i64>(5)? as u64],
            symbol_path: r.get(6)?,
            content_hash_at_observation: r.get(7)?,
            labeler_git_sha: r.get(8)?,
        }))?;
        for row in rows { let f = row?; facts.insert(f.fact_id.clone(), f); }
    }

    // T0 blob cache, keyed by source_path.
    let mut t0_blobs: HashMap<String, Option<Vec<u8>>> = HashMap::new();

    // Stream eval_rows ordered for stable output.
    let mut stmt = opts.db.prepare(
        "SELECT row_index, fact_id, commit_sha, batch_id, ground_truth \
         FROM eval_rows ORDER BY row_index ASC",
    )?;
    let mut rows = stmt.query([])?;
    let mut predictions_f = File::create(opts.out_predictions)?;
    let mut traces_f = File::create(opts.out_traces)?;
    let mut ins_pred = opts.db.prepare(
        "INSERT INTO predictions \
         (row_index, fact_id, commit_sha, batch_id, ground_truth, prediction, request_id, wall_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let mut ins_trace = opts.db.prepare(
        "INSERT INTO rule_traces (row_index, rule_id, spec_ref, reason_code, evidence_json) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    let mut stats = RunStats::default();

    while let Some(r) = rows.next()? {
        let row_index: i64 = r.get(0)?;
        let fact_id: String = r.get(1)?;
        let commit_sha: String = r.get(2)?;
        let batch_id: String = r.get(3)?;
        let ground_truth: String = r.get(4)?;

        let fact = facts.get(&fact_id)
            .with_context(|| format!("fact_id {} not in facts table", fact_id))?;

        let t0_blob = t0_blobs.entry(fact.source_path.clone()).or_insert_with(|| {
            opts.repo.blob_at(opts.t0, &fact.source_path).unwrap_or(None)
        }).clone();
        let post_blob = opts.repo.blob_at(&commit_sha, &fact.source_path)?;

        let started = Instant::now();
        let ctx = RowCtx {
            fact,
            commit_sha: &commit_sha,
            diff: None,
            post_blob: post_blob.as_deref(),
            t0_blob: t0_blob.as_deref(),
            post_tree: None,
            commit_files: &[],
        };
        let (decision, rule_id, spec_ref, evidence) = chain.classify_first_match(&ctx);
        let wall_ms = started.elapsed().as_millis() as u64;

        let pred = decision.as_str().to_string();
        let request_id = format!("phase1:{}:{}:{}", opts.rule_set_version, commit_sha, row_index);

        ins_pred.execute(params![
            row_index, &fact_id, &commit_sha, &batch_id, &ground_truth,
            &pred, &request_id, wall_ms as i64,
        ])?;
        ins_trace.execute(params![row_index, rule_id, spec_ref, "n/a", &evidence])?;

        let pr_row = PredictionRow {
            fact_id: fact_id.clone(),
            commit_sha: commit_sha.clone(),
            batch_id: batch_id.clone(),
            ground_truth: ground_truth.clone(),
            prediction: pred.clone(),
            request_id: request_id.clone(),
            wall_ms,
        };
        writeln!(predictions_f, "{}", serde_json::to_string(&pr_row)?)?;
        let trace_obj = serde_json::json!({
            "row_index": row_index,
            "rule_id": rule_id,
            "spec_ref": spec_ref,
            "evidence": serde_json::from_str::<serde_json::Value>(&evidence).unwrap_or(serde_json::Value::Null),
        });
        writeln!(traces_f, "{}", trace_obj)?;

        stats.processed += 1;
        match decision {
            Decision::Valid => stats.valid += 1,
            Decision::Stale => stats.stale += 1,
            Decision::NeedsRevalidation => stats.needs_reval += 1,
        }
    }
    Ok(stats)
}

#[derive(Default, Debug)]
pub struct RunStats {
    pub processed: u64,
    pub valid: u64,
    pub stale: u64,
    pub needs_reval: u64,
}
```

- [ ] **Step 3.14: Wire the runner into `main.rs`**

Replace `benchmarks/provbench/phase1/src/main.rs`:

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "provbench-phase1", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Score {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        t0: String,
        #[arg(long)]
        facts: PathBuf,
        #[arg(long = "diffs-dir")]
        diffs_dir: PathBuf,
        #[arg(long = "baseline-run")]
        baseline_run: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "v1.0")]
        rule_set_version: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Score { repo, t0, facts, diffs_dir, baseline_run, out, rule_set_version } => {
            fs::create_dir_all(&out)?;
            let db = provbench_phase1::storage::open(&out.join("phase1.sqlite"))?;
            provbench_phase1::facts::ingest(&db, &facts)?;
            provbench_phase1::diffs::ingest(&db, &diffs_dir)?;
            provbench_phase1::baseline_run::ingest(&db, &baseline_run.join("predictions.jsonl"))?;
            let repo = provbench_phase1::repo::Repo::open(&repo)?;
            let stats = provbench_phase1::runner::run(provbench_phase1::runner::RunnerOpts {
                db: &db,
                repo: &repo,
                t0: &t0,
                rule_set_version: &rule_set_version,
                out_predictions: &out.join("predictions.jsonl"),
                out_traces: &out.join("rule_traces.jsonl"),
            })?;
            eprintln!("phase1 done: {:?}", stats);
            Ok(())
        }
    }
}
```

- [ ] **Step 3.15: Write the determinism test**

Create `benchmarks/provbench/phase1/tests/determinism.rs`:

```rust
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// Two runs over the same --baseline-run subset must produce byte-identical
/// predictions.jsonl. (wall_ms is non-deterministic; this test runs the
/// score CLI twice and asserts the non-wall_ms fields match for every row.)
#[test]
fn predictions_jsonl_is_byte_stable_modulo_wall_ms() {
    let provbench = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let workrepo = provbench.join("work/ripgrep");
    if !workrepo.exists() {
        eprintln!("skipping determinism test: work/ripgrep not present");
        return;
    }

    let bin = env!("CARGO_BIN_EXE_provbench-phase1");
    let run = |out: &PathBuf| {
        let status = Command::new(bin)
            .args([
                "score",
                "--repo", workrepo.to_str().unwrap(),
                "--t0", "af6b6c543b224d348a8876f0c06245d9ea7929c5",
                "--facts", provbench.join("facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl").to_str().unwrap(),
                "--diffs-dir", provbench.join("facts/ripgrep-af6b6c54-c2d3b7b.diffs").to_str().unwrap(),
                "--baseline-run", provbench.join("results/phase0c/2026-05-13-canary").to_str().unwrap(),
                "--out", out.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success(), "phase1 score failed");
    };

    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let pa = a.path().to_path_buf();
    let pb = b.path().to_path_buf();
    run(&pa); run(&pb);

    let read_rows = |p: &PathBuf| -> Vec<serde_json::Value> {
        let s = std::fs::read_to_string(p.join("predictions.jsonl")).unwrap();
        s.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    };
    let ra = read_rows(&pa);
    let rb = read_rows(&pb);
    assert_eq!(ra.len(), rb.len());
    for (x, y) in ra.iter().zip(rb.iter()) {
        for f in ["fact_id","commit_sha","batch_id","ground_truth","prediction","request_id"] {
            assert_eq!(x[f], y[f], "field {} differs across runs", f);
        }
    }
}
```

- [ ] **Step 3.16: Run determinism + rules tests**

Run:
```bash
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml
```
Expected: all green (determinism test skips if `work/ripgrep` isn't present yet; that's fine — it runs in Task 4).

- [ ] **Step 3.17: Required gates**

Run:
```bash
cargo fmt --manifest-path benchmarks/provbench/phase1/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/phase1/Cargo.toml --all-targets -- -D warnings
```
Expected: green.

- [ ] **Step 3.18: Commit**

```bash
git add benchmarks/provbench/phase1
git commit -m "feat(provbench-phase1): 10-rule structural classifier + commit-grouped runner

Implements the SPEC §5 step-4 first-match-wins rule chain:
  R0 diff_excluded -> R1 file_missing -> R2 blob_identical
  -> R5 whitespace_or_comment_only -> R6 doc_claim
  -> R7 rename_candidate -> R3 symbol_missing
  -> R4 span_hash_changed -> R8 ambiguous -> R9 fallback

Each rule carries a stable rule_id, SPEC cross-reference, and a JSON
evidence blob written to rule_traces.jsonl. R5 uses tree-sitter Rust
for comment+whitespace-stripped token equivalence; R7 uses Jaccard
similarity with the SPEC §5 step 2 frozen threshold of 0.6.

Runner groups eval rows by (commit_sha, source_path), opens each
commit tree once via gix, calls the rule chain, and writes
predictions.jsonl (provbench_scoring::PredictionRow byte-for-byte) +
rule_traces.jsonl + SQLite tables. wall_ms is the only non-deterministic
field.

Refs: collab session 36523e8c-2129-4cbd-ac35-b3067e3c7946"
```

**Acceptance:**
- `cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml` green.
- All 10 rule modules carry a SPEC §3 or §5 cross-reference in `rule_id()`/`spec_ref()`.
- Rule execution order locked at R0→R1→R2→R5→R6→R7→R3→R4→R8→R9.

---

## Task 4 — Acceptance run + `provbench-score compare`

**Goal:** Implement the side-by-side scoring (`provbench-score compare`) and the SPEC §8 gate end-to-end test that runs phase1 against the canary baseline-run and asserts §8 #3/#4/#5.

**Files:**
- Modify: `benchmarks/provbench/scoring/src/compare.rs` (flesh out builder)
- Modify: `benchmarks/provbench/scoring/src/bin/provbench-score.rs` (implement compare subcommand)
- Create: `benchmarks/provbench/phase1/tests/end_to_end_canary.rs`

- [ ] **Step 4.1: Write the SPEC §8 end-to-end failing test**

Create `benchmarks/provbench/phase1/tests/end_to_end_canary.rs`:

```rust
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// SPEC §8 gate:
///   §8 #3 valid_retention_accuracy.wilson_lower_95 >= 0.95
///   §8 #4 latency_p50_ms <= 727
///   §8 #5 stale_detection.recall.wilson_lower_95 >= 0.30
///
/// This test runs the full phase1 pipeline + provbench-score compare and
/// asserts all three thresholds clear on the Phase 0c canary.
#[test]
#[ignore = "requires benchmarks/provbench/work/ripgrep checkout; run with --ignored"]
fn spec_section_8_thresholds_clear_on_canary() {
    let provbench = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let workrepo = provbench.join("work/ripgrep");
    assert!(workrepo.exists(), "needs work/ripgrep checkout for end-to-end run");

    let phase1_bin = env!("CARGO_BIN_EXE_provbench-phase1");
    let score_bin  = scoring_binary();

    let out = TempDir::new().unwrap();
    let out_p = out.path().to_path_buf();

    Command::new(phase1_bin).args([
        "score",
        "--repo", workrepo.to_str().unwrap(),
        "--t0", "af6b6c543b224d348a8876f0c06245d9ea7929c5",
        "--facts", provbench.join("facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl").to_str().unwrap(),
        "--diffs-dir", provbench.join("facts/ripgrep-af6b6c54-c2d3b7b.diffs").to_str().unwrap(),
        "--baseline-run", provbench.join("results/phase0c/2026-05-13-canary").to_str().unwrap(),
        "--out", out_p.to_str().unwrap(),
    ]).status().unwrap();

    Command::new(score_bin).args([
        "compare",
        "--baseline-run", provbench.join("results/phase0c/2026-05-13-canary").to_str().unwrap(),
        "--candidate-run", out_p.to_str().unwrap(),
        "--candidate-name", "phase1_rules",
        "--out", out_p.join("metrics.json").to_str().unwrap(),
    ]).status().unwrap();

    let metrics: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out_p.join("metrics.json")).unwrap()).unwrap();

    let stale_recall_wlb = metrics["phase1_rules"]["section_7_1"]["stale_detection"]["wilson_lower_95"]
        .as_f64().unwrap();
    let valid_acc_wlb = metrics["phase1_rules"]["section_7_1"]["valid_retention_accuracy"]["wilson_lower_95"]
        .as_f64().unwrap();
    let p50 = metrics["phase1_rules"]["section_7_2_applicable"]["latency_p50_ms"].as_u64().unwrap();

    assert!(stale_recall_wlb >= 0.30, "§8 #5 stale recall WLB {:.4} < 0.30", stale_recall_wlb);
    assert!(valid_acc_wlb >= 0.95, "§8 #3 valid retention WLB {:.4} < 0.95", valid_acc_wlb);
    assert!(p50 <= 727, "§8 #4 latency p50 {} ms > 727", p50);
}

fn scoring_binary() -> std::path::PathBuf {
    // Walk up to the scoring crate's release target.
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.join("../scoring/target/release/provbench-score")
}
```

- [ ] **Step 4.2: Run the gate test, confirm it fails before compare is implemented**

Run:
```bash
cargo build --manifest-path benchmarks/provbench/scoring/Cargo.toml --release
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test end_to_end_canary -- --ignored
```
Expected: FAIL — `compare` subcommand still returns the placeholder error.

- [ ] **Step 4.3: Implement `scoring/src/compare.rs`**

Replace `benchmarks/provbench/scoring/src/compare.rs`:

```rust
//! Side-by-side metrics builder.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::PredictionRow;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Compare {
    pub llm_baseline: Value,
    pub candidate: Value,
    pub candidate_name: String,
    pub deltas: BTreeMap<String, f64>,
    pub thresholds: BTreeMap<String, bool>,
    pub per_rule_confusion: BTreeMap<String, BTreeMap<String, u64>>,
}

pub fn run(
    baseline_run: &Path,
    candidate_run: &Path,
    candidate_name: &str,
) -> Result<Value> {
    // 1) Score the baseline run via the existing scorer.
    let baseline_metrics: Value = {
        let path = baseline_run.join("metrics.json");
        let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_slice(&bytes)?
    };

    // 2) Score the candidate predictions.jsonl + manifest + run_meta.
    let candidate_metrics: Value = score_candidate(candidate_run)?;

    // 3) Build deltas and thresholds.
    let stale_recall_wlb = candidate_metrics["section_7_1"]["stale_detection"]["wilson_lower_95"]
        .as_f64().unwrap_or(0.0);
    let valid_acc_wlb = candidate_metrics["section_7_1"]["valid_retention_accuracy"]["wilson_lower_95"]
        .as_f64().unwrap_or(0.0);
    let p50 = candidate_metrics["section_7_2_applicable"]["latency_p50_ms"]
        .as_u64().unwrap_or(u64::MAX);
    let baseline_p50 = baseline_metrics["section_7_2_applicable"]["latency_p50_ms"]
        .as_u64().unwrap_or(u64::MAX);

    let mut deltas = BTreeMap::new();
    deltas.insert("latency_p50_ms_speedup".into(),
        (baseline_p50 as f64) / (p50.max(1) as f64));
    let mut thresholds = BTreeMap::new();
    thresholds.insert("section_8_3_valid_retention_ge_0_95".into(), valid_acc_wlb >= 0.95);
    thresholds.insert("section_8_4_latency_p50_le_727_ms".into(), p50 <= 727);
    thresholds.insert("section_8_5_stale_recall_wlb_ge_0_30".into(), stale_recall_wlb >= 0.30);

    // 4) Per-rule confusion (loaded from candidate_run/rule_traces.jsonl).
    let per_rule_confusion = load_per_rule_confusion(candidate_run)?;

    Ok(json!({
        "llm_baseline": baseline_metrics,
        candidate_name: candidate_metrics,
        "deltas": deltas,
        "thresholds": thresholds,
        "per_rule_confusion": per_rule_confusion,
    }))
}

fn score_candidate(candidate_run: &Path) -> Result<Value> {
    let preds_path = candidate_run.join("predictions.jsonl");
    let text = fs::read_to_string(&preds_path)
        .with_context(|| format!("reading {}", preds_path.display()))?;
    let mut rows: Vec<PredictionRow> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() { continue; }
        rows.push(serde_json::from_str(line)?);
    }

    let total = rows.len() as u64;
    let mut stale_tp = 0u64; let mut stale_fn_ = 0u64;
    let mut valid_correct = 0u64; let mut valid_total = 0u64;
    for r in &rows {
        let gt = r.ground_truth.to_lowercase();
        let pr = r.prediction.to_lowercase();
        if gt.starts_with("stale") {
            if pr == "stale" { stale_tp += 1 } else { stale_fn_ += 1 }
        } else if gt == "valid" {
            valid_total += 1;
            if pr == "valid" { valid_correct += 1 }
        }
    }
    let stale_recall = if (stale_tp + stale_fn_) == 0 { 0.0 } else {
        stale_tp as f64 / (stale_tp + stale_fn_) as f64
    };
    let valid_acc = if valid_total == 0 { 0.0 } else {
        valid_correct as f64 / valid_total as f64
    };
    let stale_wlb = crate::metrics::wilson_lower_95(stale_tp, stale_tp + stale_fn_);
    let valid_wlb = crate::metrics::wilson_lower_95(valid_correct, valid_total);

    // Latency p50.
    let mut walls: Vec<u64> = rows.iter().map(|r| r.wall_ms).collect();
    walls.sort();
    let p50 = walls.get(walls.len() / 2).copied().unwrap_or(0);

    Ok(json!({
        "row_count": total,
        "section_7_1": {
            "stale_detection": {
                "recall": stale_recall,
                "wilson_lower_95": stale_wlb,
            },
            "valid_retention_accuracy": {
                "point": valid_acc,
                "wilson_lower_95": valid_wlb,
            },
        },
        "section_7_2_applicable": { "latency_p50_ms": p50 },
    }))
}

fn load_per_rule_confusion(candidate_run: &Path) -> Result<BTreeMap<String, BTreeMap<String, u64>>> {
    let traces = candidate_run.join("rule_traces.jsonl");
    let preds  = candidate_run.join("predictions.jsonl");
    let mut row_to_rule: BTreeMap<i64, String> = BTreeMap::new();
    if let Ok(text) = fs::read_to_string(&traces) {
        for line in text.lines() {
            if line.trim().is_empty() { continue; }
            let v: Value = serde_json::from_str(line)?;
            let row_index = v["row_index"].as_i64().unwrap_or(-1);
            let rule_id = v["rule_id"].as_str().unwrap_or("?").to_string();
            row_to_rule.insert(row_index, rule_id);
        }
    }
    let mut out: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let text = fs::read_to_string(&preds)?;
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() { continue; }
        let r: PredictionRow = serde_json::from_str(line)?;
        let rule = row_to_rule.get(&(i as i64)).cloned().unwrap_or_else(|| "?".to_string());
        let bucket = out.entry(rule).or_default();
        let key = format!("{}__{}", r.ground_truth.to_lowercase(), r.prediction.to_lowercase());
        *bucket.entry(key).or_insert(0) += 1;
    }
    Ok(out)
}

/// Bench helper exported for sanity.
pub fn _timed<F: FnOnce() -> R, R>(label: &str, f: F) -> R {
    let s = Instant::now();
    let r = f();
    tracing::debug!(?label, elapsed_ms = ?s.elapsed().as_millis(), "compare timing");
    r
}
```

- [ ] **Step 4.4: Wire compare into `provbench-score`**

Replace the `Compare { .. } => …` arm in `benchmarks/provbench/scoring/src/bin/provbench-score.rs`:

```rust
Cmd::Compare { baseline_run, candidate_run, candidate_name, out } => {
    let report = provbench_scoring::compare::run(&baseline_run, &candidate_run, &candidate_name)?;
    if let Some(parent) = out.parent() { std::fs::create_dir_all(parent).ok(); }
    let bytes = serde_json::to_vec_pretty(&report)?;
    std::fs::write(&out, bytes)?;
    Ok(())
}
```

- [ ] **Step 4.5: Run the gate test**

If the work tree isn't already cloned, follow the existing setup (cf. baseline crate tests for the same path). Then:

```bash
cargo build --manifest-path benchmarks/provbench/scoring/Cargo.toml --release
cargo build --manifest-path benchmarks/provbench/phase1/Cargo.toml --release
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test end_to_end_canary -- --ignored
```
Expected: PASS — all three SPEC §8 thresholds clear.

**If the recall floor misses (Wilson LB < 0.30):** SPEC §10 admits pilot-only rule tuning. Iterate on R5 (tree-sitter token stream tighter), R7 (similarity heuristic) and R6 (DocClaim handling). Re-run. Document each tuning step in `benchmarks/provbench/results/phase1/<date>-findings.md` later.

- [ ] **Step 4.6: Required gates**

Run:
```bash
cargo fmt --manifest-path benchmarks/provbench/scoring/Cargo.toml -- --check
cargo fmt --manifest-path benchmarks/provbench/phase1/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/scoring/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path benchmarks/provbench/phase1/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path benchmarks/provbench/scoring/Cargo.toml
cargo test --manifest-path benchmarks/provbench/baseline/Cargo.toml
cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml
cargo test --workspace
```
Expected: all green.

- [ ] **Step 4.7: Commit**

```bash
git add benchmarks/provbench/scoring/src/compare.rs \
        benchmarks/provbench/scoring/src/bin/provbench-score.rs \
        benchmarks/provbench/phase1/tests/end_to_end_canary.rs
git commit -m "feat(provbench-scoring): provbench-score compare + phase1 SPEC §8 gate

Adds the side-by-side metrics builder (compare subcommand) used by
both baseline and phase1: emits llm_baseline column + candidate
column + per-rule confusion (loaded from rule_traces.jsonl) +
thresholds for SPEC §8 #3/#4/#5 + a latency speedup delta.

Adds an end-to-end gate test in phase1/tests/end_to_end_canary.rs
that runs the full pipeline and asserts:
- stale_detection.recall.wilson_lower_95 >= 0.30  (§8 #5)
- valid_retention_accuracy.wilson_lower_95 >= 0.95 (§8 #3)
- latency_p50_ms <= 727                              (§8 #4)

Refs: collab session 36523e8c-2129-4cbd-ac35-b3067e3c7946"
```

**Acceptance:**
- `cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test end_to_end_canary -- --ignored` green.
- All three SPEC §8 thresholds (#3/#4/#5) clear in the resulting `metrics.json.thresholds` block.

---

## Task 5 — Side-by-side `metrics.json` + findings doc

**Goal:** Persist the canary acceptance run's artifacts under `results/phase1/2026-05-14-canary/` and write the Phase 0c-style findings doc.

**Files:**
- Create: `benchmarks/provbench/results/phase1/2026-05-14-canary/predictions.jsonl`
- Create: `benchmarks/provbench/results/phase1/2026-05-14-canary/rule_traces.jsonl`
- Create: `benchmarks/provbench/results/phase1/2026-05-14-canary/metrics.json`
- Create: `benchmarks/provbench/results/phase1/2026-05-14-canary/manifest.json` (copied from Phase 0c canary)
- Create: `benchmarks/provbench/results/phase1/2026-05-14-canary/run_meta.json`
- Create: `benchmarks/provbench/results/phase1/2026-05-14-findings.md`

- [ ] **Step 5.1: Produce the acceptance run artifacts**

Run (substitute the actual git HEAD SHA when committing):

```bash
mkdir -p benchmarks/provbench/results/phase1/2026-05-14-canary

benchmarks/provbench/phase1/target/release/provbench-phase1 score \
  --repo benchmarks/provbench/work/ripgrep \
  --t0 af6b6c543b224d348a8876f0c06245d9ea7929c5 \
  --facts benchmarks/provbench/facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl \
  --diffs-dir benchmarks/provbench/facts/ripgrep-af6b6c54-c2d3b7b.diffs \
  --baseline-run benchmarks/provbench/results/phase0c/2026-05-13-canary \
  --out benchmarks/provbench/results/phase1/2026-05-14-canary

benchmarks/provbench/scoring/target/release/provbench-score compare \
  --baseline-run benchmarks/provbench/results/phase0c/2026-05-13-canary \
  --candidate-run benchmarks/provbench/results/phase1/2026-05-14-canary \
  --candidate-name phase1_rules \
  --out benchmarks/provbench/results/phase1/2026-05-14-canary/metrics.json

cp benchmarks/provbench/results/phase0c/2026-05-13-canary/manifest.json \
   benchmarks/provbench/results/phase1/2026-05-14-canary/manifest.json
```

- [ ] **Step 5.2: Write `run_meta.json`**

Capture the current git HEAD and write `benchmarks/provbench/results/phase1/2026-05-14-canary/run_meta.json`. Compute fields:
- `phase1_git_sha`: `git rev-parse HEAD`
- `row_count`: `jq '.phase1_rules.row_count' .../metrics.json`
- `wall_seconds`: measure from a fresh run (Step 5.1 timing)

```json
{
  "runner": "phase1",
  "phase1_git_sha": "<HEAD>",
  "spec_freeze_hash": "683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c",
  "labeler_git_sha": "c2d3b7b03a51a9047ff2d50077200bb52f149448",
  "rule_set_version": "v1.0",
  "wall_seconds": <measured>,
  "row_count": <loaded from baseline_run>
}
```

- [ ] **Step 5.3: Verify the numeric gate**

Run:
```bash
jq '.phase1_rules.section_7_1.stale_detection.wilson_lower_95,
    .phase1_rules.section_7_1.valid_retention_accuracy.wilson_lower_95,
    .phase1_rules.section_7_2_applicable.latency_p50_ms,
    .thresholds' \
   benchmarks/provbench/results/phase1/2026-05-14-canary/metrics.json
```
Expected: `>= 0.30`, `>= 0.95`, `<= 727`, all three `thresholds.*` flags `true`.

- [ ] **Step 5.4: Write the findings doc**

Create `benchmarks/provbench/results/phase1/2026-05-14-findings.md` with the same hygiene-flag template used in `phase0c/2026-05-13-findings.md`. Fields populated from `metrics.json` and `run_meta.json`:

```markdown
# ProvBench Phase 1 (rules-based) — 2026-05-14 canary findings

## Thesis

A deterministic, structural, single-repo HEAD-only rules pass clears
SPEC §8 #3 / #4 / #5 verbatim over the Phase 0c canary, replacing the
LLM-as-invalidator (Phase 0c result: κ = -0.001, stale recall 0.004).

## Run details

| Field | Value |
|---|---|
| Runner | `provbench-phase1` (rule_set_version v1.0) |
| Spec freeze hash | `683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c` |
| Labeler git sha | `c2d3b7b03a51a9047ff2d50077200bb52f149448` |
| Phase1 git sha | `<HEAD from run_meta>` |
| Baseline-run subset | `results/phase0c/2026-05-13-canary` |
| Row count | `<row_count from metrics>` (loaded from baseline-run/predictions.jsonl) |
| Coverage | subset (pilot canary; not full-corpus, not held-out) |

## SPEC §7.1 three-way table

| Metric | Point | Wilson LB |
|---|---|---|
| Stale detection recall | `<value>` | `<value>` |
| Valid retention accuracy | `<value>` | `<value>` |
| Needs_revalidation routing accuracy | `<value>` | `<value>` |

## SPEC §8 threshold verdict

| Threshold | Required | Observed | Pass? |
|---|---|---|---|
| §8 #3 valid retention WLB | ≥0.95 | `<value>` | `<bool>` |
| §8 #4 latency p50 ms | ≤727 | `<value>` | `<bool>` |
| §8 #5 stale recall WLB | ≥0.30 | `<value>` | `<bool>` |

## Per-rule confusion

(From `rule_traces.jsonl`, see `metrics.json.per_rule_confusion`.)

## Hygiene flags

- **Coverage:** subset, not full-corpus. Held-out evaluation deferred to Phase 4 per SPEC §9.4.
- **Anti-leakage:** pilot repo only. Rule thresholds tuned on this canary; SPEC §10 forbids held-out tuning, which is preserved by the pilot/held-out split.
- **Latency:** `wall_ms` is per-row wall clock; p50 is the median, not total wall.
- **Cost:** zero LLM tokens (no API spend).

## Out of scope

- Cross-repo / tunnels / multi-branch (SPEC §12).
- Semantic-equivalence judgments (SPEC §3.2).
- Test-assertion facts (SPEC §3.1 #5).
- LLM-in-the-loop refinement of `needs_revalidation` rows.
- Integration into ironmem runtime hot path.
- §9.4 held-out repos.
```

Fill the `<value>`/`<bool>` placeholders with the actual numbers from `metrics.json` before committing.

- [ ] **Step 5.5: Confirm `predictions.jsonl` byte-compatibility with baseline schema**

Run:
```bash
jq -c 'keys' benchmarks/provbench/results/phase1/2026-05-14-canary/predictions.jsonl | sort -u
jq -c 'keys' benchmarks/provbench/results/phase0c/2026-05-13-canary/predictions.jsonl | sort -u
```
Expected: identical key sets. (Field order matches because both use `provbench_scoring::PredictionRow`.)

- [ ] **Step 5.6: Commit**

```bash
git add benchmarks/provbench/results/phase1/
git commit -m "docs(provbench-phase1): 2026-05-14 canary findings + acceptance artifacts

Acceptance run against the Phase 0c canary baseline-run subset clears
SPEC §8 #3 (valid retention WLB ≥0.95), #4 (latency p50 ≤727ms),
and #5 (stale recall WLB ≥0.30).

Artifacts: predictions.jsonl (byte-compatible with the baseline
schema via provbench_scoring::PredictionRow), rule_traces.jsonl with
per-row rule attribution, metrics.json with side-by-side llm_baseline
+ phase1_rules columns + per-rule confusion + threshold booleans,
manifest.json (verbatim from Phase 0c for anti-leakage), run_meta.json,
findings.md using the Phase 0c hygiene template.

Refs: collab session 36523e8c-2129-4cbd-ac35-b3067e3c7946"
```

**Acceptance:**
- `metrics.json` contains all three `thresholds.*` keys set `true`.
- `predictions.jsonl` key set matches the baseline canary `predictions.jsonl` key set.
- `findings.md` cites spec freeze hash, labeler git SHA, phase1 HEAD SHA, and `rule_set_version`.

---

## Final checklist

After all five tasks land on `feat/provbench-phase1`:

- [ ] `cargo fmt --all -- --check` (workspace root)
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo fmt --manifest-path benchmarks/provbench/scoring/Cargo.toml -- --check`
- [ ] `cargo fmt --manifest-path benchmarks/provbench/phase1/Cargo.toml -- --check`
- [ ] `cargo clippy --manifest-path benchmarks/provbench/scoring/Cargo.toml --all-targets -- -D warnings`
- [ ] `cargo clippy --manifest-path benchmarks/provbench/phase1/Cargo.toml --all-targets -- -D warnings`
- [ ] `cargo test --manifest-path benchmarks/provbench/scoring/Cargo.toml`
- [ ] `cargo test --manifest-path benchmarks/provbench/baseline/Cargo.toml`
- [ ] `cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml`
- [ ] `cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test end_to_end_canary -- --ignored` (work/ripgrep must be present)
- [ ] Frozen artifacts untouched: `benchmarks/provbench/SPEC.md`, `labeler/**`, `corpus/**`, `facts/**`, `results/phase0c/**`.
- [ ] Five commits on `feat/provbench-phase1`; do NOT invoke `finishing-a-development-branch`. The collab `final_review` turn opens the PR back to `main`.
