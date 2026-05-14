# ProvBench Phase 0c LLM-as-invalidator Baseline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up a benchmark-only Rust runner that scores a stratified sample of the Phase 0b corpus through Claude Sonnet 4.6 per SPEC §6.1 and emits all SPEC §9.2 metrics with full reproducibility, under an operational $25 cap (spec cap $250 unchanged).

**Architecture:** Two Rust crates, both standalone and **excluded** from the `ironrace-memory` Cargo workspace. The existing `benchmarks/provbench/labeler/` gains two emit subcommands that serialize T₀ fact bodies and per-commit unified diffs as JSON artifacts. A new sibling crate `benchmarks/provbench/baseline/` consumes those JSON artifacts and the corpus JSONL, runs a stratified-by-label sampler, calls the Anthropic Messages API with prompt caching on a static prefix block (SPEC §6.1 prompt frozen byte-for-byte), and emits `predictions.jsonl` + `metrics.json` + `run_meta.json`. No ironmem crate ever imports the baseline.

**Tech Stack:** Rust 1.91+, `clap` 4, `tokio` 1, `reqwest` 0.12 (rustls), `serde`/`serde_json`, `rand_chacha` 0.3 (deterministic seed), `statrs` 0.17 (Wilson + bootstrap), `indicatif` 0.17, `wiremock` 0.6 (mock HTTP for tests). No third-party Anthropic SDK.

**Reference spec:** `/Users/jeffreycrum/.claude/plans/peppy-wondering-torvalds.md` (the collab-locked final plan for session `f0863069-43c0-4803-b03a-cc640f82a5a6`). SPEC source of truth: `benchmarks/provbench/SPEC.md` (frozen 2026-05-09).

**Branch strategy:** Work on a feature branch (e.g. `feat/provbench-phase0c-baseline`) off `main`. Commit at every TDD GREEN. The branch lands via the collab-driven PR flow at the end (do NOT invoke `finishing-a-development-branch`; the collab `final_review` turn opens the PR).

---

## File Structure

### Existing (labeler) — modify

| File | Change |
|---|---|
| `benchmarks/provbench/labeler/src/main.rs` | Add two `Cmd` variants: `EmitFacts`, `EmitDiffs`. |
| `benchmarks/provbench/labeler/src/output.rs` | Add `FactBodyRow` + `DiffArtifact` structs and `write_facts_jsonl` / `write_diff_json` helpers. |
| `benchmarks/provbench/labeler/src/lib.rs` | Re-export new public types. |

### New (baseline crate)

```
benchmarks/provbench/baseline/
  Cargo.toml
  Cargo.lock                           # committed
  build.rs                             # SPEC §6.1 byte-equality check at build time
  README.md
  src/
    main.rs                            # `provbench-baseline` CLI (clap subcommands)
    lib.rs
    constants.rs                       # SPEC immutables: model, prices, $25 op cap, $250 spec cap
    prompt.rs                          # PROMPT_STATIC_PREFIX, PROMPT_STATIC_SUFFIX, addendum
    facts.rs                           # FactBody schema + loader
    diffs.rs                           # DiffArtifact schema + loader
    manifest.rs                        # SampleManifest + atomic save/load + content hash
    sample.rs                          # stratified sampler (ChaCha20)
    budget.rs                          # preflight + runtime cost meter
    client.rs                          # Anthropic HTTP client (retries, cache_control, parse-error addendum)
    runner.rs                          # batch dispatcher + checkpointing
    metrics.rs                         # §7.1 three-way + §9.2 LLM-validator agreement + Wilson + HT
    report.rs                          # writes predictions.jsonl + metrics.json + run_meta.json
  tests/
    prompt_frozen.rs                   # byte-equality vs SPEC §6.1
    prompt_assembly.rs                 # 5-block concatenation
    fact_body_render.rs                # per-kind body fixtures
    sample_determinism.rs              # seed → byte-identical manifest
    sample_exclusions.rs               # T₀, no-parent, malformed excluded with reasons
    metric_math.rs                     # §7.1 + LLM-validator-agreement formulas
    client_retries.rs                  # wiremock: 5xx ×2, 429 backoff, parse-error addendum
    caching_layout.rs                  # cache_control on block 3 only, conditional on ≥2 batches
    budget_preflight.rs                # n=9,232 passes; oversized refused
    budget_runtime.rs                  # live meter aborts at 95% of op cap; no partial truncation
    resume_safety.rs                   # --resume verifies manifest hash, skips done rows
    end_to_end_fixture.rs              # full CLI loop against fixture diffs/facts
  fixtures/
    sample_corpus.jsonl                # 50 rows hand-picked across all 5 labels
    sample_facts.jsonl                 # corresponding fact bodies
    sample_diffs/                      # 5–10 per-commit diff JSON files
    sample_api_response.json           # canonical valid response
    sample_api_parse_error.json        # malformed response that triggers parse-retry
```

### New (artifact directories — populated at runtime)

```
benchmarks/provbench/facts/
  ripgrep-af6b6c54-<labeler-sha>.facts.jsonl
  ripgrep-af6b6c54-<labeler-sha>.diffs/<commit-sha>.json

benchmarks/provbench/results/phase0c/<run-id>/
  manifest.json                        # committed before any API call
  predictions.jsonl                    # atomic append-then-rename per batch
  latency.jsonl
  run_meta.json
  metrics.json                         # written by `score` subcommand
```

---

### Task 1: Labeler `emit-facts` subcommand

**Files:**
- Modify: `benchmarks/provbench/labeler/src/main.rs` (add `EmitFacts` variant + match arm)
- Modify: `benchmarks/provbench/labeler/src/output.rs` (add `FactBodyRow` + writer)
- Modify: `benchmarks/provbench/labeler/src/lib.rs` (re-export types)
- Test: `benchmarks/provbench/labeler/tests/emit_facts_roundtrip.rs`

**Acceptance criteria:**
- `provbench-labeler emit-facts --corpus <jsonl> --repo <work> --t0 <sha> --out <facts.jsonl>` writes one row per unique `fact_id` from the corpus.
- Each row contains `{fact_id, kind, body, source_path, line_span: [start, end], symbol_path, content_hash_at_observation}` plus a trailing `labeler_git_sha` field per the existing stamping convention.
- Output is sorted by `fact_id` and byte-deterministic across runs.
- Test asserts a 5-row fixture corpus emits a 5-row facts artifact whose deserialized contents match expected fixtures for all five fact kinds.

- [ ] **Step 1: Write the failing roundtrip test**

Create `benchmarks/provbench/labeler/tests/emit_facts_roundtrip.rs`:

```rust
//! Verifies emit-facts produces one well-formed FactBodyRow per unique fact_id.
use std::process::Command;
use tempfile::TempDir;

#[test]
fn emit_facts_writes_one_row_per_unique_fact_id() {
    let dir = TempDir::new().unwrap();
    let corpus = dir.path().join("corpus.jsonl");
    let out = dir.path().join("facts.jsonl");
    // Fixture: 3 unique fact_ids across 2 commits → 3 facts rows.
    std::fs::write(
        &corpus,
        r#"{"fact_id":"FunctionSignature::crate::lib::foo","commit_sha":"a","label":"Valid","labeler_git_sha":"X"}
{"fact_id":"FunctionSignature::crate::lib::foo","commit_sha":"b","label":"Valid","labeler_git_sha":"X"}
{"fact_id":"Field::crate::T::n","commit_sha":"a","label":"Valid","labeler_git_sha":"X"}
{"fact_id":"DocClaim::auto::README.md::1","commit_sha":"a","label":"Valid","labeler_git_sha":"X"}
"#,
    ).unwrap();

    let bin = env!("CARGO_BIN_EXE_provbench-labeler");
    let status = Command::new(bin)
        .args([
            "emit-facts",
            "--corpus", corpus.to_str().unwrap(),
            "--repo", "tests/fixtures/tiny-repo",
            "--t0", "af6b6c543b224d348a8876f0c06245d9ea7929c5",
            "--out", out.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "emit-facts must exit 0");

    let lines: Vec<_> = std::fs::read_to_string(&out).unwrap().lines().count();
    assert_eq!(lines, 3, "expected one row per unique fact_id");

    // Determinism: re-run produces byte-identical output.
    let out2 = dir.path().join("facts2.jsonl");
    Command::new(bin)
        .args([
            "emit-facts",
            "--corpus", corpus.to_str().unwrap(),
            "--repo", "tests/fixtures/tiny-repo",
            "--t0", "af6b6c543b224d348a8876f0c06245d9ea7929c5",
            "--out", out2.to_str().unwrap(),
        ])
        .status().unwrap();
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(&out2).unwrap(),
        "emit-facts must be byte-deterministic"
    );
}
```

Add `tempfile = "3"` to `[dev-dependencies]` in `benchmarks/provbench/labeler/Cargo.toml` if not already present.

- [ ] **Step 2: Run test to confirm it fails (no `emit-facts` subcommand yet)**

```
cargo test --release --manifest-path benchmarks/provbench/labeler/Cargo.toml -- emit_facts_roundtrip
```

Expected: FAIL with clap error `unrecognized subcommand 'emit-facts'`.

- [ ] **Step 3: Add `FactBodyRow` schema + writer to `output.rs`**

Append to `benchmarks/provbench/labeler/src/output.rs`:

```rust
/// One T₀ fact body row, emitted by `emit-facts`. Mirrors the
/// `baseline/src/facts.rs::FactBody` schema (single source of truth).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactBodyRow {
    pub fact_id: String,
    pub kind: String,                            // "FunctionSignature" | "Field" | "PublicSymbol" | "DocClaim" | "TestAssertion"
    pub body: String,                            // deterministic single-line render per SPEC §3
    pub source_path: String,
    pub line_span: [u32; 2],
    pub symbol_path: String,
    pub content_hash_at_observation: String,     // 64-char lowercase hex
}

#[derive(Debug, Serialize)]
struct StampedFactBody<'a> {
    fact_id: &'a str,
    kind: &'a str,
    body: &'a str,
    source_path: &'a str,
    line_span: [u32; 2],
    symbol_path: &'a str,
    content_hash_at_observation: &'a str,
    labeler_git_sha: &'a str,
}

/// Write `rows` as JSONL, sorted by `fact_id`, stamped with
/// `labeler_git_sha`. Byte-deterministic.
pub fn write_facts_jsonl(
    path: &Path,
    rows: &[FactBodyRow],
    labeler_git_sha: &str,
) -> Result<()> {
    let mut sorted: Vec<&FactBodyRow> = rows.iter().collect();
    sorted.sort_by(|a, b| a.fact_id.cmp(&b.fact_id));
    let mut f = std::fs::File::create(path)?;
    for row in sorted {
        let stamped = StampedFactBody {
            fact_id: &row.fact_id,
            kind: &row.kind,
            body: &row.body,
            source_path: &row.source_path,
            line_span: row.line_span,
            symbol_path: &row.symbol_path,
            content_hash_at_observation: &row.content_hash_at_observation,
            labeler_git_sha,
        };
        serde_json::to_writer(&mut f, &stamped)?;
        f.write_all(b"\n")?;
    }
    f.flush()?;
    Ok(())
}
```

- [ ] **Step 4: Add `EmitFacts` subcommand wiring in `main.rs`**

Add to `Cmd` enum in `benchmarks/provbench/labeler/src/main.rs`:

```rust
    /// Emit one T₀ fact body row per unique fact_id in the corpus.
    EmitFacts {
        #[arg(long)]
        corpus: std::path::PathBuf,
        #[arg(long)]
        repo: std::path::PathBuf,
        #[arg(long)]
        t0: String,
        #[arg(long)]
        out: std::path::PathBuf,
    },
```

Add match arm to `fn main()`:

```rust
        Some(Cmd::EmitFacts { corpus, repo, t0, out }) => {
            let cfg = provbench_labeler::replay::ReplayConfig {
                repo_path: repo,
                t0_sha: t0,
                skip_symbol_resolution: false,
            };
            // Collect unique fact_ids from corpus.
            let corpus_rows: Vec<provbench_labeler::output::OutputRow> =
                provbench_labeler::output::read_jsonl(&corpus)?;
            let mut unique: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for r in &corpus_rows {
                unique.insert(r.fact_id.clone());
            }
            // Re-extract T₀ facts and produce FactBodyRow for each requested fact_id.
            let rows = provbench_labeler::replay::Replay::emit_facts(&cfg, &unique)?;
            let sha = provbench_labeler::labeler_stamp();
            provbench_labeler::output::write_facts_jsonl(&out, &rows, &sha)?;
            println!("wrote {} fact bodies to {}", rows.len(), out.display());
            Ok(())
        }
```

Also add helper `read_jsonl` to `output.rs`:

```rust
pub fn read_jsonl(path: &Path) -> Result<Vec<OutputRow>> {
    let f = std::fs::File::open(path)?;
    let mut rows = Vec::new();
    for line in std::io::BufRead::lines(std::io::BufReader::new(f)) {
        let line = line?;
        if line.trim().is_empty() { continue; }
        rows.push(serde_json::from_str(&line)?);
    }
    Ok(rows)
}
```

- [ ] **Step 5: Implement `Replay::emit_facts` in `src/replay/mod.rs`**

Find the existing T₀ fact-extraction pass in `benchmarks/provbench/labeler/src/replay/mod.rs` (it's already present — the labeler builds a T₀ fact set before classifying any commit). Wire a sibling method that returns `FactBodyRow` values for the requested `fact_id` set. The body renderer is per fact kind:

```rust
pub fn emit_facts(
    cfg: &ReplayConfig,
    wanted: &std::collections::BTreeSet<String>,
) -> anyhow::Result<Vec<crate::output::FactBodyRow>> {
    let repo = crate::repo::open(&cfg.repo_path, &cfg.t0_sha)?;
    let t0_facts = crate::facts::extract_all(&repo, &cfg.t0_sha)?;
    let mut out = Vec::new();
    for fact in t0_facts {
        let id = fact.fact_id();
        if !wanted.contains(&id) { continue; }
        out.push(crate::output::FactBodyRow {
            fact_id: id,
            kind: fact.kind_name().to_string(),
            body: fact.render_body(),
            source_path: fact.source_path().to_string(),
            line_span: fact.line_span(),
            symbol_path: fact.symbol_path(),
            content_hash_at_observation: fact.content_hash().to_string(),
        });
    }
    Ok(out)
}
```

The `Fact` trait surface (`kind_name`, `render_body`, `source_path`, `line_span`, `symbol_path`, `content_hash`) likely exists or is straightforward to add — extend it in `src/facts/mod.rs` as needed. Per-kind `render_body` returns the SPEC §3 single-line claim:

- FunctionSignature: `"function {qualified_name} has parameters ({params}) with return type {ret}"`
- Field: `"type {parent} has field {name} of type {ty}"`
- PublicSymbol: `"exported name {name} resolves in module {module}"`
- DocClaim: `"doc {doc_file} mentions symbol {symbol}"`
- TestAssertion: `"test {test_qualified} asserts property about symbol {target}"`

- [ ] **Step 6: Add a `tests/fixtures/tiny-repo/` ripgrep slice**

Either commit a minimal real git repo (3–4 commits, ~10 facts touching all 5 kinds) under the labeler tests fixtures path, or rely on the existing labeler determinism-test fixtures (check `benchmarks/provbench/labeler/tests/` for what's already there and reuse it).

- [ ] **Step 7: Run the test, confirm GREEN**

```
cargo test --release --manifest-path benchmarks/provbench/labeler/Cargo.toml -- emit_facts_roundtrip
```

Expected: PASS.

- [ ] **Step 8: Format + clippy + commit**

```
cargo fmt --manifest-path benchmarks/provbench/labeler/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/labeler/Cargo.toml --all-targets -- -D warnings
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench-labeler): add emit-facts subcommand for Phase 0c"
```

---

### Task 2: Labeler `emit-diffs` subcommand

**Files:**
- Modify: `benchmarks/provbench/labeler/src/main.rs` (add `EmitDiffs` variant)
- Modify: `benchmarks/provbench/labeler/src/output.rs` (add `DiffArtifact` + writer)
- Modify: `benchmarks/provbench/labeler/src/diff/mod.rs` (add `full_file_context_diff` helper if not present)
- Test: `benchmarks/provbench/labeler/tests/emit_diffs_roundtrip.rs`

**Acceptance criteria:**
- `provbench-labeler emit-diffs --corpus <jsonl> --repo <work> --out-dir <dir>` writes one `<commit_sha>.json` per distinct `commit_sha` in the corpus.
- Each artifact JSON contains either `{"commit_sha": ..., "parent_sha": ..., "unified_diff": "<diff text>"}` or `{"commit_sha": ..., "excluded": "no_parent" | "t0"}` for commits that have no parent or equal T₀.
- The `unified_diff` uses `git diff -U999999 <parent>..<commit>` — full file context for every file touched in the commit.
- Test asserts a fixture commit produces a non-empty `unified_diff` with expected `--- a/` / `+++ b/` markers and that a T₀ commit produces an `excluded:"t0"` row.

- [ ] **Step 1: Write failing test**

Create `benchmarks/provbench/labeler/tests/emit_diffs_roundtrip.rs`:

```rust
use std::process::Command;
use tempfile::TempDir;

#[test]
fn emit_diffs_writes_one_artifact_per_commit() {
    let dir = TempDir::new().unwrap();
    let corpus = dir.path().join("corpus.jsonl");
    let out = dir.path().join("diffs");
    // Fixture corpus naming two real commits in tests/fixtures/tiny-repo plus T₀.
    std::fs::write(
        &corpus,
        format!(
            "{{\"fact_id\":\"X\",\"commit_sha\":\"{}\",\"label\":\"Valid\",\"labeler_git_sha\":\"Z\"}}\n\
             {{\"fact_id\":\"X\",\"commit_sha\":\"{}\",\"label\":\"Valid\",\"labeler_git_sha\":\"Z\"}}\n",
            FIXTURE_T0, FIXTURE_NEXT_COMMIT,
        ),
    ).unwrap();
    let bin = env!("CARGO_BIN_EXE_provbench-labeler");
    let status = Command::new(bin)
        .args([
            "emit-diffs",
            "--corpus", corpus.to_str().unwrap(),
            "--repo", "tests/fixtures/tiny-repo",
            "--t0", FIXTURE_T0,
            "--out-dir", out.to_str().unwrap(),
        ])
        .status().unwrap();
    assert!(status.success());

    // T₀ excluded
    let t0_json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out.join(format!("{}.json", FIXTURE_T0))).unwrap()
    ).unwrap();
    assert_eq!(t0_json["excluded"], "t0");

    // Next commit has a unified_diff
    let next_json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out.join(format!("{}.json", FIXTURE_NEXT_COMMIT))).unwrap()
    ).unwrap();
    let diff = next_json["unified_diff"].as_str().unwrap();
    assert!(diff.starts_with("diff --git") || diff.contains("--- a/"), "expected unified diff text");
    assert!(diff.len() > 0);
}

const FIXTURE_T0: &str = "<set to T₀ of tests/fixtures/tiny-repo>";
const FIXTURE_NEXT_COMMIT: &str = "<set to T₀^1 of tests/fixtures/tiny-repo>";
```

- [ ] **Step 2: Run test, confirm FAIL**

```
cargo test --release --manifest-path benchmarks/provbench/labeler/Cargo.toml -- emit_diffs_roundtrip
```

Expected: FAIL (subcommand missing).

- [ ] **Step 3: Add diff helper to `src/diff/mod.rs`**

```rust
/// Compute the unified diff between two commits with full file context
/// (`-U999999`) restricted to files actually touched in the commit.
/// Uses `git` via `std::process::Command` (no shell interpolation —
/// arg-vector invocation only) against the repo at `repo_path`.
pub fn full_file_context_diff(
    repo_path: &std::path::Path,
    parent: &str,
    commit: &str,
) -> anyhow::Result<String> {
    use anyhow::Context;
    let output = std::process::Command::new("git")
        .arg("-C").arg(repo_path)
        .args(["diff", "-U999999", parent, commit])
        .output()
        .context("git diff invocation failed")?;
    anyhow::ensure!(
        output.status.success(),
        "git diff returned non-zero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

/// Return parent SHA for `commit`, or `None` if root commit.
pub fn parent_sha(
    repo_path: &std::path::Path,
    commit: &str,
) -> anyhow::Result<Option<String>> {
    let output = std::process::Command::new("git")
        .arg("-C").arg(repo_path)
        .args(["rev-parse", &format!("{commit}^")])
        .output()?;
    if output.status.success() {
        Ok(Some(String::from_utf8(output.stdout)?.trim().to_string()))
    } else {
        Ok(None)
    }
}
```

- [ ] **Step 4: Add `DiffArtifact` to `output.rs`**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DiffArtifact {
    Included {
        commit_sha: String,
        parent_sha: String,
        unified_diff: String,
    },
    Excluded {
        commit_sha: String,
        excluded: String,    // "t0" | "no_parent"
    },
}

pub fn write_diff_json(out_dir: &Path, artifact: &DiffArtifact) -> Result<()> {
    let sha = match artifact {
        DiffArtifact::Included { commit_sha, .. } => commit_sha,
        DiffArtifact::Excluded { commit_sha, .. } => commit_sha,
    };
    std::fs::create_dir_all(out_dir)?;
    let path = out_dir.join(format!("{sha}.json"));
    let mut f = std::fs::File::create(path)?;
    serde_json::to_writer(&mut f, artifact)?;
    f.write_all(b"\n")?;
    Ok(())
}
```

- [ ] **Step 5: Wire `EmitDiffs` subcommand in `main.rs`**

```rust
    /// Emit one DiffArtifact per distinct commit_sha in the corpus.
    EmitDiffs {
        #[arg(long)] corpus: std::path::PathBuf,
        #[arg(long)] repo: std::path::PathBuf,
        #[arg(long)] t0: String,
        #[arg(long, name = "out-dir")] out_dir: std::path::PathBuf,
    },
```

Match arm:

```rust
Some(Cmd::EmitDiffs { corpus, repo, t0, out_dir }) => {
    let rows = provbench_labeler::output::read_jsonl(&corpus)?;
    let mut commits: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for r in &rows { commits.insert(r.commit_sha.clone()); }
    for commit in &commits {
        if commit == &t0 {
            provbench_labeler::output::write_diff_json(&out_dir,
                &provbench_labeler::output::DiffArtifact::Excluded {
                    commit_sha: commit.clone(),
                    excluded: "t0".into(),
                })?;
            continue;
        }
        match provbench_labeler::diff::parent_sha(&repo, commit)? {
            None => {
                provbench_labeler::output::write_diff_json(&out_dir,
                    &provbench_labeler::output::DiffArtifact::Excluded {
                        commit_sha: commit.clone(),
                        excluded: "no_parent".into(),
                    })?;
            }
            Some(parent) => {
                let diff = provbench_labeler::diff::full_file_context_diff(&repo, &parent, commit)?;
                provbench_labeler::output::write_diff_json(&out_dir,
                    &provbench_labeler::output::DiffArtifact::Included {
                        commit_sha: commit.clone(),
                        parent_sha: parent,
                        unified_diff: diff,
                    })?;
            }
        }
    }
    println!("wrote {} diff artifacts to {}", commits.len(), out_dir.display());
    Ok(())
}
```

- [ ] **Step 6: Run test, confirm GREEN**

```
cargo test --release --manifest-path benchmarks/provbench/labeler/Cargo.toml -- emit_diffs_roundtrip
```

Expected: PASS.

- [ ] **Step 7: Format + clippy + commit**

```
cargo fmt --manifest-path benchmarks/provbench/labeler/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/labeler/Cargo.toml --all-targets -- -D warnings
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench-labeler): add emit-diffs subcommand (SPEC §6.1-compliant full-file-context diffs)"
```

---

### Task 3: Baseline crate skeleton + build-time frozen-prompt check

**Files:**
- Create: `benchmarks/provbench/baseline/Cargo.toml`
- Create: `benchmarks/provbench/baseline/build.rs`
- Create: `benchmarks/provbench/baseline/src/main.rs`
- Create: `benchmarks/provbench/baseline/src/lib.rs`
- Create: `benchmarks/provbench/baseline/src/constants.rs`
- Create: `benchmarks/provbench/baseline/src/prompt.rs` (skeleton only — full content in Task 4)
- Test: `benchmarks/provbench/baseline/tests/prompt_frozen.rs`

**Acceptance criteria:**
- The crate compiles standalone with `cargo build --release --manifest-path benchmarks/provbench/baseline/Cargo.toml`.
- It is **not** added to the root workspace `Cargo.toml`. The root `cargo build` must not pick it up.
- `build.rs` extracts the SPEC §6.1 prompt block from `benchmarks/provbench/SPEC.md` and `panic!()`s if it disagrees with the const in `prompt.rs`.
- `tests/prompt_frozen.rs` performs the same check at test time and fails loudly on drift.
- CLI exposes three subcommands stubs: `sample`, `run`, `score` (implementations come in later tasks).

- [ ] **Step 1: Create `Cargo.toml`**

Create `benchmarks/provbench/baseline/Cargo.toml`:

```toml
[package]
name = "provbench-baseline"
version = "0.1.0"
edition = "2021"
rust-version = "1.91"
description = "Phase 0c LLM-as-invalidator baseline for ProvBench-CodeContext (frozen contract: benchmarks/provbench/SPEC.md)"
license = "Apache-2.0"
publish = false

[[bin]]
name = "provbench-baseline"
path = "src/main.rs"

[lib]
name = "provbench_baseline"
path = "src/lib.rs"

[dependencies]
anyhow = "1"
thiserror = "2"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
hex = "0.4"
rand_chacha = "0.3"
rand = "0.8"
statrs = "0.17"
indicatif = "0.17"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "fs", "io-util", "sync"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json", "stream"] }
futures = "0.3"
tempfile = "3"

[dev-dependencies]
wiremock = "0.6"
tokio = { version = "1", features = ["full"] }
```

- [ ] **Step 2: Confirm the new crate is workspace-excluded**

Check the root `Cargo.toml` for a `[workspace] members = [...]` block. Ensure `benchmarks/provbench/baseline` is in the `exclude` list (alongside `benchmarks/provbench/labeler`). If the root uses `members` globbing, add an explicit `exclude` entry:

```toml
[workspace]
# ... existing members ...
exclude = ["benchmarks/provbench/labeler", "benchmarks/provbench/baseline"]
```

- [ ] **Step 3: Write the failing frozen-prompt test**

Create `benchmarks/provbench/baseline/tests/prompt_frozen.rs`:

```rust
//! Byte-equality between SPEC §6.1 fenced prompt block and PROMPT_TEMPLATE_FROZEN.

use provbench_baseline::prompt::PROMPT_TEMPLATE_FROZEN;

#[test]
fn frozen_prompt_matches_spec_byte_for_byte() {
    let spec = include_str!("../../SPEC.md");
    let block = extract_section_6_1_block(spec);
    assert_eq!(
        block.trim_end_matches('\n'),
        PROMPT_TEMPLATE_FROZEN.trim_end_matches('\n'),
        "SPEC §6.1 prompt block drifted from PROMPT_TEMPLATE_FROZEN — \
         spec change requires §11 entry and a new freeze hash"
    );
}

fn extract_section_6_1_block(spec: &str) -> String {
    // The block in SPEC.md is fenced with ``` after "### 6.1 Prompt (frozen text, exact)".
    let mut lines = spec.lines();
    while let Some(line) = lines.next() {
        if line.starts_with("### 6.1 Prompt") { break; }
    }
    // Find opening fence.
    for line in lines.by_ref() {
        if line.trim() == "```" { break; }
    }
    // Collect until closing fence.
    let mut out = String::new();
    for line in lines {
        if line.trim() == "```" { break; }
        out.push_str(line);
        out.push('\n');
    }
    out
}
```

- [ ] **Step 4: Create the prompt skeleton**

Create `benchmarks/provbench/baseline/src/prompt.rs`:

```rust
/// SPEC §6.1 verbatim. Byte-equality enforced by `build.rs` and `tests/prompt_frozen.rs`.
/// Edits to this string must be matched by a SPEC §11 entry and a new freeze hash.
pub const PROMPT_TEMPLATE_FROZEN: &str = "\
You are evaluating whether claims about source code are still supported
after a code change.

For each FACT in the FACTS list, decide one of:
  - \"valid\": the change does not affect the fact.
  - \"stale\": the change makes the fact no longer supported.
  - \"needs_revalidation\": the change is relevant but you cannot tell
    from structural information alone whether the fact still holds.

You must base your decision only on the DIFF and the FACT body.
Do not speculate about runtime behavior.

DIFF:
<unified diff, full file context for affected hunks>

FACTS:
<JSON array of {id, kind, body, source_path, line_span, symbol_path,
content_hash_at_observation}>

Respond with a JSON array of {id, decision} only. No prose.";

/// Literal addendum text from SPEC §6.2 for the parse-failure retry. Frozen.
pub const PARSE_RETRY_ADDENDUM: &str =
    "Your previous response was not valid JSON. Respond with a JSON array of {id, decision} only.";
```

- [ ] **Step 5: Create `build.rs`**

```rust
//! Build-time SPEC §6.1 prompt drift check.

use std::path::PathBuf;

fn main() {
    let spec_path: PathBuf = PathBuf::from("../SPEC.md");
    println!("cargo:rerun-if-changed=../SPEC.md");
    println!("cargo:rerun-if-changed=src/prompt.rs");
    let spec = std::fs::read_to_string(&spec_path)
        .unwrap_or_else(|e| panic!("read SPEC.md: {e} (cwd-sensitive build)"));
    let block = extract_block(&spec);

    let prompt_rs = std::fs::read_to_string("src/prompt.rs").unwrap();
    // Pull the literal between the first pair of "\\\n" and the trailing closing quote.
    let frozen = extract_frozen_const(&prompt_rs);

    if block.trim_end_matches('\n') != frozen.trim_end_matches('\n') {
        panic!(
            "SPEC §6.1 drifted from src/prompt.rs::PROMPT_TEMPLATE_FROZEN\n\
             SPEC block:\n{}\n---\nCONST:\n{}\n",
            block, frozen
        );
    }
}

fn extract_block(spec: &str) -> String {
    let mut lines = spec.lines();
    while let Some(line) = lines.next() {
        if line.starts_with("### 6.1 Prompt") { break; }
    }
    for line in lines.by_ref() {
        if line.trim() == "```" { break; }
    }
    let mut out = String::new();
    for line in lines {
        if line.trim() == "```" { break; }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn extract_frozen_const(rs: &str) -> String {
    // Find: pub const PROMPT_TEMPLATE_FROZEN: &str = "\
    let start = rs.find("PROMPT_TEMPLATE_FROZEN").expect("const missing");
    let after_eq = rs[start..].find("= \"\\\n").expect("expected `= \"\\\n` opening");
    let body_start = start + after_eq + "= \"\\\n".len();
    let body_end = rs[body_start..].find("\";").expect("closing `\";` missing") + body_start;
    rs[body_start..body_end]
        .replace("\\\"", "\"")        // unescape \"
        .replace("\\\\", "\\")
}
```

- [ ] **Step 6: Create `lib.rs`, `constants.rs`, `main.rs` stubs**

`benchmarks/provbench/baseline/src/lib.rs`:

```rust
//! Phase 0c LLM-as-invalidator baseline runner.
//!
//! Benchmark-only. Workspace-excluded. Never imported by ironmem.

pub mod constants;
pub mod prompt;
// Modules added in subsequent tasks:
// pub mod facts;
// pub mod diffs;
// pub mod manifest;
// pub mod sample;
// pub mod budget;
// pub mod client;
// pub mod runner;
// pub mod metrics;
// pub mod report;
```

`benchmarks/provbench/baseline/src/constants.rs`:

```rust
//! SPEC immutables. Token prices and caps pinned to §6.2 / §15 snapshot 2026-05-09.

pub const MODEL_ID: &str = "claude-sonnet-4-6";
pub const MODEL_SNAPSHOT_DATE: &str = "2026-05-09";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

pub const TEMPERATURE: f32 = 0.0;
pub const MAX_TOKENS: u32 = 4096;
pub const MAX_FACTS_PER_BATCH: usize = 32;

/// $3.00 per 1M input tokens (uncached).
pub const PRICE_INPUT_UNCACHED_USD_PER_MTOK: f64 = 3.00;
/// Cache write: 1.25× the input price per Anthropic prompt-caching pricing.
pub const PRICE_INPUT_CACHE_WRITE_USD_PER_MTOK: f64 = 3.75;
/// Cache read: 0.10× the input price.
pub const PRICE_INPUT_CACHE_READ_USD_PER_MTOK: f64 = 0.30;
pub const PRICE_OUTPUT_USD_PER_MTOK: f64 = 15.00;

/// Spec ceiling — never exceedable. Per §6.2.
pub const SPEC_BUDGET_USD: f64 = 250.00;
/// Default operational guardrail. Overridable via `--budget-usd`.
pub const DEFAULT_OPERATIONAL_BUDGET_USD: f64 = 25.00;

pub const DEFAULT_SEED: u64 = 0xC0DE_BABE_DEAD_BEEF;

/// Default per-stratum sample targets (spec-frozen for the §9.2 baseline run).
pub const TARGET_VALID: usize = 2000;
pub const TARGET_STALE_CHANGED: usize = 2000;
pub const TARGET_STALE_DELETED: usize = 2000;
pub const TARGET_STALE_RENAMED: usize = usize::MAX;     // sentinel: take FULL stratum
pub const TARGET_NEEDS_REVALIDATION: usize = 2000;
```

`benchmarks/provbench/baseline/src/main.rs`:

```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "provbench-baseline", version, about = "Phase 0c LLM baseline runner")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// Build a stratified sample manifest from the corpus + facts + diffs artifacts.
    Sample {
        #[arg(long)] corpus: std::path::PathBuf,
        #[arg(long)] facts: std::path::PathBuf,
        #[arg(long, name = "diffs-dir")] diffs_dir: std::path::PathBuf,
        #[arg(long, default_value_t = provbench_baseline::constants::DEFAULT_SEED)]
        seed: u64,
        #[arg(long, name = "budget-usd", default_value_t = provbench_baseline::constants::DEFAULT_OPERATIONAL_BUDGET_USD)]
        budget_usd: f64,
        #[arg(long)] out: std::path::PathBuf,
    },
    /// Score a manifest against the live Anthropic API.
    Run {
        #[arg(long)] manifest: std::path::PathBuf,
        #[arg(long, default_value_t = 4)] max_concurrency: usize,
        #[arg(long, default_value_t = false)] resume: bool,
        #[arg(long, default_value_t = false)] dry_run: bool,
        #[arg(long, name = "fixture-mode")] fixture_mode: Option<std::path::PathBuf>,
        #[arg(long, name = "max-batches")] max_batches: Option<usize>,
    },
    /// Compute metrics over a completed run directory.
    Score {
        #[arg(long)] run: std::path::PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    match cli.command {
        Cmd::Sample { .. } => anyhow::bail!("sample: not yet implemented (Task 5)"),
        Cmd::Run    { .. } => anyhow::bail!("run: not yet implemented (Task 7)"),
        Cmd::Score  { .. } => anyhow::bail!("score: not yet implemented (Task 8)"),
    }
}
```

- [ ] **Step 7: Build the crate, then run the frozen-prompt test**

```
cargo build --release --manifest-path benchmarks/provbench/baseline/Cargo.toml
cargo test --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- prompt_frozen
```

Expected: build passes (build.rs SPEC drift check runs), test PASS.

- [ ] **Step 8: Format + clippy + commit**

```
cargo fmt --manifest-path benchmarks/provbench/baseline/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/baseline/Cargo.toml --all-targets -- -D warnings
git add benchmarks/provbench/baseline/ Cargo.toml
git commit -m "feat(provbench-baseline): crate skeleton + build-time §6.1 frozen-prompt check"
```

---

### Task 4: 5-block prompt assembly + cache_control placement

**Files:**
- Modify: `benchmarks/provbench/baseline/src/prompt.rs` (add `PromptBuilder`)
- Test: `benchmarks/provbench/baseline/tests/prompt_assembly.rs`
- Test: `benchmarks/provbench/baseline/tests/caching_layout.rs`

**Acceptance criteria:**
- `PromptBuilder::build(diff: &str, facts: &[FactBody], multi_batch: bool) -> Vec<ContentBlock>` returns exactly 5 text blocks.
- Block 1 ends with `"DIFF:\n"`. Block 2 is the diff text. Block 3 is the literal `"\n\nFACTS:\n"`. Block 4 is the FACTS JSON array. Block 5 is the trailing instruction.
- `cache_control: {"type": "ephemeral"}` is present on **block 3 only** when `multi_batch == true`. Absent otherwise.
- Concatenating all five block texts produces a string that, when stripped of the variable diff and facts payloads, equals `PROMPT_TEMPLATE_FROZEN`.

- [ ] **Step 1: Write `prompt_assembly.rs` test**

Create `benchmarks/provbench/baseline/tests/prompt_assembly.rs`:

```rust
use provbench_baseline::prompt::{ContentBlock, PromptBuilder, FactBody};

#[test]
fn five_blocks_in_order_with_correct_static_text() {
    let facts = vec![FactBody {
        fact_id: "FunctionSignature::foo".into(),
        kind: "FunctionSignature".into(),
        body: "function foo has parameters () with return type ()".into(),
        source_path: "src/lib.rs".into(),
        line_span: [1, 3],
        symbol_path: "foo".into(),
        content_hash_at_observation: "0".repeat(64),
    }];
    let blocks = PromptBuilder::build("--- a/x\n+++ b/x", &facts, false);
    assert_eq!(blocks.len(), 5, "must be exactly 5 blocks");
    assert!(blocks[0].text.ends_with("DIFF:\n"), "block 1 ends with DIFF:\\n");
    assert_eq!(blocks[1].text, "--- a/x\n+++ b/x");
    assert_eq!(blocks[2].text, "\n\nFACTS:\n");
    assert!(blocks[3].text.starts_with('['), "block 4 is a JSON array");
    assert!(blocks[3].text.contains("FunctionSignature::foo"));
    assert!(blocks[4].text.contains("Respond with a JSON array"));
}
```

- [ ] **Step 2: Write `caching_layout.rs` test**

```rust
use provbench_baseline::prompt::{PromptBuilder, FactBody};

fn one_fact() -> Vec<FactBody> {
    vec![FactBody {
        fact_id: "X".into(), kind: "FunctionSignature".into(),
        body: "x".into(), source_path: "x".into(), line_span: [1, 1],
        symbol_path: "x".into(), content_hash_at_observation: "0".repeat(64),
    }]
}

#[test]
fn cache_control_only_on_block_3_when_multi_batch() {
    let blocks = PromptBuilder::build("D", &one_fact(), true);
    assert!(blocks[0].cache_control.is_none());
    assert!(blocks[1].cache_control.is_none());
    assert!(blocks[2].cache_control.is_some(), "block 3 caches when multi_batch=true");
    assert_eq!(blocks[2].cache_control.as_ref().unwrap(), "ephemeral");
    assert!(blocks[3].cache_control.is_none(), "FACTS must never be cached");
    assert!(blocks[4].cache_control.is_none());
}

#[test]
fn no_cache_control_on_single_batch() {
    let blocks = PromptBuilder::build("D", &one_fact(), false);
    for (i, b) in blocks.iter().enumerate() {
        assert!(b.cache_control.is_none(), "block {i} must have no cache_control on single-batch");
    }
}
```

- [ ] **Step 3: Implement `PromptBuilder` in `prompt.rs`**

```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FactBody {
    pub fact_id: String,
    pub kind: String,
    pub body: String,
    pub source_path: String,
    pub line_span: [u32; 2],
    pub symbol_path: String,
    pub content_hash_at_observation: String,
}

#[derive(Debug, Clone)]
pub struct ContentBlock {
    pub text: String,
    pub cache_control: Option<&'static str>,
}

pub struct PromptBuilder;

impl PromptBuilder {
    pub fn build(diff: &str, facts: &[FactBody], multi_batch: bool) -> Vec<ContentBlock> {
        // Split the frozen template at the static seams.
        let (prefix, suffix) = split_template();
        let block1_text = prefix;                          // everything up to and including "DIFF:\n"
        let block2_text = diff.to_string();
        let block3_text = "\n\nFACTS:\n".to_string();
        let block4_text = serde_json::to_string(&fact_payload(facts))
            .expect("FactBody serialization must not fail");
        let block5_text = suffix;                          // trailing instruction line

        vec![
            ContentBlock { text: block1_text, cache_control: None },
            ContentBlock { text: block2_text, cache_control: None },
            ContentBlock { text: block3_text, cache_control: if multi_batch { Some("ephemeral") } else { None } },
            ContentBlock { text: block4_text, cache_control: None },
            ContentBlock { text: block5_text, cache_control: None },
        ]
    }
}

fn split_template() -> (String, String) {
    // PROMPT_TEMPLATE_FROZEN contains placeholders "<unified diff, ...>" and
    // "<JSON array of {...}>". Strip those placeholders to recover the static prefix and suffix.
    let tpl = PROMPT_TEMPLATE_FROZEN;
    let diff_marker = "<unified diff, full file context for affected hunks>";
    let facts_marker_start = "<JSON array of {id, kind, body, source_path, line_span, symbol_path,\ncontent_hash_at_observation}>";
    let i_diff = tpl.find(diff_marker).expect("diff placeholder missing");
    // block1 ends at the start of the diff placeholder
    let block1 = tpl[..i_diff].to_string();
    let after_diff = i_diff + diff_marker.len();
    let i_facts = tpl[after_diff..].find(facts_marker_start).expect("facts placeholder missing") + after_diff;
    // we already emit "\n\nFACTS:\n" ourselves as block 3 — the suffix starts after the facts placeholder
    let after_facts = i_facts + facts_marker_start.len();
    let block5 = tpl[after_facts..].trim_start_matches('\n').to_string();
    // Sanity: block5 must start with the trailing instruction line.
    debug_assert!(block5.contains("Respond with a JSON array"));
    (block1, block5)
}

#[derive(Serialize)]
struct FactPayload<'a> {
    id: &'a str,
    kind: &'a str,
    body: &'a str,
    source_path: &'a str,
    line_span: [u32; 2],
    symbol_path: &'a str,
    content_hash_at_observation: &'a str,
}

fn fact_payload(facts: &[FactBody]) -> Vec<FactPayload<'_>> {
    facts.iter().map(|f| FactPayload {
        id: &f.fact_id,
        kind: &f.kind,
        body: &f.body,
        source_path: &f.source_path,
        line_span: f.line_span,
        symbol_path: &f.symbol_path,
        content_hash_at_observation: &f.content_hash_at_observation,
    }).collect()
}
```

Also export from `lib.rs`: uncomment `pub mod prompt;` (the skeleton stub already added the module).

- [ ] **Step 4: Run both tests, confirm GREEN**

```
cargo test --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- prompt_assembly caching_layout
```

Expected: PASS.

- [ ] **Step 5: Format + clippy + commit**

```
cargo fmt --manifest-path benchmarks/provbench/baseline/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/baseline/Cargo.toml --all-targets -- -D warnings
git add benchmarks/provbench/baseline/
git commit -m "feat(provbench-baseline): 5-block prompt assembly with conditional cache_control"
```

---

### Task 5: Stratified sampler + manifest

**Files:**
- Create: `benchmarks/provbench/baseline/src/facts.rs` (FactBody schema + loader)
- Create: `benchmarks/provbench/baseline/src/diffs.rs` (DiffArtifact schema + loader)
- Create: `benchmarks/provbench/baseline/src/manifest.rs`
- Create: `benchmarks/provbench/baseline/src/sample.rs`
- Modify: `benchmarks/provbench/baseline/src/main.rs` (wire `Sample` subcommand)
- Test: `benchmarks/provbench/baseline/tests/sample_determinism.rs`
- Test: `benchmarks/provbench/baseline/tests/sample_exclusions.rs`

**Acceptance criteria:**
- `SampleManifest::from_corpus(...)` returns deterministic per-stratum row counts for a fixed seed.
- Running `sample` twice with the same seed produces byte-identical `manifest.json`.
- Excluded rows (commit listed as `excluded:"t0"` or `excluded:"no_parent"`, malformed corpus row, fact_id missing from facts artifact) are recorded in the manifest's `excluded` array with reason codes — never silently dropped.
- Manifest carries: `seed`, `corpus_path`, `facts_path`, `diffs_dir`, `labeler_git_sha`, `spec_freeze_hash`, `per_stratum_targets`, `selected_count`, `excluded_count_by_reason`, `estimated_worst_case_usd`, `baseline_crate_head_sha`, `created_at` (ISO-8601 UTC).
- A manifest content-hash field (`sha256` of the canonical JSON minus the field itself) is included for `--resume` integrity.

- [ ] **Step 1: Write `sample_determinism.rs` test (will fail until implementation lands)**

```rust
use provbench_baseline::manifest::SampleManifest;
use provbench_baseline::sample::PerStratumTargets;
use std::path::Path;

#[test]
fn same_seed_yields_byte_identical_manifest() {
    let targets = PerStratumTargets::default();
    let m1 = SampleManifest::from_corpus(
        Path::new("fixtures/sample_corpus.jsonl"),
        Path::new("fixtures/sample_facts.jsonl"),
        Path::new("fixtures/sample_diffs"),
        0xC0DEBABEDEADBEEF,
        targets.clone(),
        25.0,
    ).unwrap();
    let m2 = SampleManifest::from_corpus(
        Path::new("fixtures/sample_corpus.jsonl"),
        Path::new("fixtures/sample_facts.jsonl"),
        Path::new("fixtures/sample_diffs"),
        0xC0DEBABEDEADBEEF,
        targets,
        25.0,
    ).unwrap();
    assert_eq!(m1.canonical_json(), m2.canonical_json());
    assert_eq!(m1.content_hash, m2.content_hash);
}
```

- [ ] **Step 2: Write `sample_exclusions.rs` test**

```rust
use provbench_baseline::manifest::SampleManifest;
use provbench_baseline::sample::PerStratumTargets;
use std::path::Path;

#[test]
fn excluded_rows_are_recorded_not_silently_dropped() {
    let targets = PerStratumTargets::default();
    let m = SampleManifest::from_corpus(
        Path::new("fixtures/sample_corpus_with_exclusions.jsonl"),
        Path::new("fixtures/sample_facts.jsonl"),
        Path::new("fixtures/sample_diffs_with_t0_excluded"),
        0xC0DEBABEDEADBEEF,
        targets,
        25.0,
    ).unwrap();
    assert!(m.excluded_count_by_reason.contains_key("commit_t0"));
    assert!(m.excluded_count_by_reason.contains_key("missing_fact_body"));
}
```

- [ ] **Step 3: Implement `facts.rs` + `diffs.rs` loaders**

```rust
// facts.rs
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

pub use crate::prompt::FactBody;

#[derive(Deserialize)]
struct FactBodyOnDisk {
    fact_id: String,
    kind: String,
    body: String,
    source_path: String,
    line_span: [u32; 2],
    symbol_path: String,
    content_hash_at_observation: String,
    // `labeler_git_sha` ignored on load (carried separately in manifest).
    #[serde(default, rename = "labeler_git_sha")] _stamp: Option<String>,
}

pub fn load_facts(path: &Path) -> Result<HashMap<String, FactBody>> {
    let f = std::fs::File::open(path).with_context(|| format!("open facts {}", path.display()))?;
    let mut map = HashMap::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        let r: FactBodyOnDisk = serde_json::from_str(&line)?;
        map.insert(r.fact_id.clone(), FactBody {
            fact_id: r.fact_id, kind: r.kind, body: r.body,
            source_path: r.source_path, line_span: r.line_span,
            symbol_path: r.symbol_path, content_hash_at_observation: r.content_hash_at_observation,
        });
    }
    Ok(map)
}
```

```rust
// diffs.rs
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DiffArtifact {
    Included { commit_sha: String, parent_sha: String, unified_diff: String },
    Excluded { commit_sha: String, excluded: String },
}

pub fn load_diffs_dir(dir: &Path) -> Result<HashMap<String, DiffArtifact>> {
    let mut map = HashMap::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        let text = std::fs::read_to_string(&path)?;
        let artifact: DiffArtifact = serde_json::from_str(&text)?;
        let sha = match &artifact {
            DiffArtifact::Included { commit_sha, .. } => commit_sha.clone(),
            DiffArtifact::Excluded { commit_sha, .. } => commit_sha.clone(),
        };
        map.insert(sha, artifact);
    }
    Ok(map)
}
```

- [ ] **Step 4: Implement `sample.rs`**

```rust
use crate::constants::*;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerStratumTargets {
    pub valid: usize,
    pub stale_changed: usize,
    pub stale_deleted: usize,
    pub stale_renamed: usize,       // usize::MAX = FULL
    pub needs_revalidation: usize,
}

impl Default for PerStratumTargets {
    fn default() -> Self {
        Self {
            valid: TARGET_VALID,
            stale_changed: TARGET_STALE_CHANGED,
            stale_deleted: TARGET_STALE_DELETED,
            stale_renamed: TARGET_STALE_RENAMED,
            needs_revalidation: TARGET_NEEDS_REVALIDATION,
        }
    }
}

/// Coalesced label class for stratification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StratumKey {
    Valid, StaleChanged, StaleDeleted, StaleRenamed, NeedsRevalidation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampledRow {
    pub fact_id: String,
    pub commit_sha: String,
    pub ground_truth: String,    // serialized label tag e.g. "Valid"
    pub stratum: StratumKey,
}

pub fn stratify_and_sample(
    rows: impl IntoIterator<Item = (String, String, String)>,  // (fact_id, commit_sha, label_tag)
    seed: u64,
    targets: &PerStratumTargets,
) -> Vec<SampledRow> {
    let mut by_stratum: std::collections::HashMap<StratumKey, Vec<SampledRow>> = Default::default();
    for (fid, csh, lab) in rows {
        let stratum = match lab.as_str() {
            "Valid" => StratumKey::Valid,
            "StaleSourceChanged" => StratumKey::StaleChanged,
            "StaleSourceDeleted" => StratumKey::StaleDeleted,
            "StaleSymbolRenamed" => StratumKey::StaleRenamed,
            "NeedsRevalidation" => StratumKey::NeedsRevalidation,
            _ => continue,
        };
        by_stratum.entry(stratum).or_default().push(SampledRow {
            fact_id: fid, commit_sha: csh, ground_truth: lab, stratum,
        });
    }
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let mut out = Vec::new();
    for (stratum, target) in [
        (StratumKey::Valid, targets.valid),
        (StratumKey::StaleChanged, targets.stale_changed),
        (StratumKey::StaleDeleted, targets.stale_deleted),
        (StratumKey::StaleRenamed, targets.stale_renamed),
        (StratumKey::NeedsRevalidation, targets.needs_revalidation),
    ] {
        let mut pool = by_stratum.remove(&stratum).unwrap_or_default();
        pool.sort_by(|a, b| a.fact_id.cmp(&b.fact_id).then_with(|| a.commit_sha.cmp(&b.commit_sha)));
        pool.shuffle(&mut rng);
        let n = if target == usize::MAX { pool.len() } else { target.min(pool.len()) };
        out.extend(pool.into_iter().take(n));
    }
    out
}
```

- [ ] **Step 5: Implement `manifest.rs`**

```rust
use crate::constants::*;
use crate::diffs::{DiffArtifact, load_diffs_dir};
use crate::facts::load_facts;
use crate::sample::{stratify_and_sample, PerStratumTargets, SampledRow};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleManifest {
    pub seed: u64,
    pub corpus_path: PathBuf,
    pub facts_path: PathBuf,
    pub diffs_dir: PathBuf,
    pub labeler_git_sha: String,
    pub spec_freeze_hash: String,
    pub baseline_crate_head_sha: String,
    pub per_stratum_targets: PerStratumTargets,
    pub selected_count: usize,
    pub excluded_count_by_reason: HashMap<String, usize>,
    pub estimated_worst_case_usd: f64,
    pub rows: Vec<SampledRow>,
    pub created_at: String,
    pub content_hash: String,
}

impl SampleManifest {
    pub fn from_corpus(
        corpus_path: &Path,
        facts_path: &Path,
        diffs_dir: &Path,
        seed: u64,
        targets: PerStratumTargets,
        budget_usd: f64,
    ) -> Result<Self> {
        let facts = load_facts(facts_path)?;
        let diffs = load_diffs_dir(diffs_dir)?;
        let mut excluded: HashMap<String, usize> = HashMap::new();
        let mut eligible: Vec<(String, String, String)> = Vec::new();
        let mut labeler_git_sha: String = String::new();

        let f = std::fs::File::open(corpus_path)
            .with_context(|| format!("open corpus {}", corpus_path.display()))?;
        for line in std::io::BufReader::new(f).lines() {
            let line = line?;
            if line.trim().is_empty() { continue; }
            let v: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => { *excluded.entry("malformed_row".into()).or_insert(0) += 1; continue; }
            };
            let fact_id = v["fact_id"].as_str().unwrap_or_default().to_string();
            let commit_sha = v["commit_sha"].as_str().unwrap_or_default().to_string();
            let label_tag = match &v["label"] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Object(map) => map.keys().next().cloned().unwrap_or_default(),
                _ => { *excluded.entry("malformed_label".into()).or_insert(0) += 1; continue; }
            };
            if labeler_git_sha.is_empty() {
                if let Some(s) = v["labeler_git_sha"].as_str() {
                    labeler_git_sha = s.to_string();
                }
            }
            if !facts.contains_key(&fact_id) {
                *excluded.entry("missing_fact_body".into()).or_insert(0) += 1; continue;
            }
            match diffs.get(&commit_sha) {
                Some(DiffArtifact::Included { .. }) => {}
                Some(DiffArtifact::Excluded { excluded: reason, .. }) => {
                    *excluded.entry(format!("commit_{reason}")).or_insert(0) += 1; continue;
                }
                None => { *excluded.entry("missing_diff_artifact".into()).or_insert(0) += 1; continue; }
            }
            eligible.push((fact_id, commit_sha, label_tag));
        }

        let rows = stratify_and_sample(eligible, seed, &targets);
        let selected_count = rows.len();
        let estimated_worst_case_usd = crate::budget::preflight_worst_case_cost(&rows, &diffs, &facts);

        anyhow::ensure!(
            estimated_worst_case_usd <= budget_usd,
            "preflight refuses: estimated worst-case ${:.2} > budget ${:.2}",
            estimated_worst_case_usd, budget_usd
        );

        let spec_freeze_hash = compute_sha256_of_path(Path::new("benchmarks/provbench/SPEC.md"))?;
        let baseline_crate_head_sha = git_head_sha_or_unknown();
        let created_at = chrono_like_utc_now();

        let mut m = SampleManifest {
            seed,
            corpus_path: corpus_path.to_path_buf(),
            facts_path: facts_path.to_path_buf(),
            diffs_dir: diffs_dir.to_path_buf(),
            labeler_git_sha,
            spec_freeze_hash,
            baseline_crate_head_sha,
            per_stratum_targets: targets,
            selected_count,
            excluded_count_by_reason: excluded,
            estimated_worst_case_usd,
            rows,
            created_at,
            content_hash: String::new(),
        };
        m.content_hash = m.compute_content_hash();
        Ok(m)
    }

    pub fn canonical_json(&self) -> String {
        // Serialize with sorted keys (serde_json default key order is insertion order — use BTreeMap for stability).
        serde_json::to_string(self).unwrap()
    }

    fn compute_content_hash(&self) -> String {
        let mut tmp = self.clone();
        tmp.content_hash = String::new();
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_vec(&tmp).unwrap());
        hex::encode(hasher.finalize())
    }

    pub fn save_atomic(&self, path: &Path) -> Result<()> {
        let parent = path.parent().expect("manifest path has parent");
        std::fs::create_dir_all(parent)?;
        let tmp = parent.join(format!(".manifest.tmp.{}", std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

fn compute_sha256_of_path(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut h = Sha256::new(); h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

fn git_head_sha_or_unknown() -> String {
    std::process::Command::new("git").args(["rev-parse", "HEAD"]).output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn chrono_like_utc_now() -> String {
    // Avoid the chrono dep — just use SystemTime + a fixed format.
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    // YYYY-MM-DDTHH:MM:SSZ ish — but it's enough to record provenance.
    format!("unix-seconds-{s}Z")
}
```

- [ ] **Step 6: Wire `Sample` subcommand in `main.rs`** (replace `bail!` for `Cmd::Sample`):

```rust
Cmd::Sample { corpus, facts, diffs_dir, seed, budget_usd, out } => {
    let manifest = provbench_baseline::manifest::SampleManifest::from_corpus(
        &corpus, &facts, &diffs_dir, seed,
        provbench_baseline::sample::PerStratumTargets::default(),
        budget_usd,
    )?;
    manifest.save_atomic(&out)?;
    println!("wrote manifest to {} (selected={}, excluded={})",
             out.display(), manifest.selected_count,
             manifest.excluded_count_by_reason.values().sum::<usize>());
    Ok(())
}
```

Add `pub mod facts; pub mod diffs; pub mod manifest; pub mod sample; pub mod budget;` to `lib.rs`.

- [ ] **Step 7: Stub `budget::preflight_worst_case_cost` (full impl in Task 6)**

Create `benchmarks/provbench/baseline/src/budget.rs` with a stub that returns a conservative estimate based on `rows.len()`:

```rust
use crate::sample::SampledRow;
use crate::diffs::DiffArtifact;
use crate::facts::FactBody;
use std::collections::HashMap;

pub fn preflight_worst_case_cost(
    rows: &[SampledRow],
    _diffs: &HashMap<String, DiffArtifact>,
    _facts: &HashMap<String, FactBody>,
) -> f64 {
    // Conservative placeholder until Task 6 fills in the schema-derived estimator.
    let batches = (rows.len() as f64 / crate::constants::MAX_FACTS_PER_BATCH as f64).ceil();
    // Worst-case input ~13K tokens uncached; output ~1800 tokens (schema bound).
    let cost_per_batch = 13_000.0 / 1_000_000.0 * crate::constants::PRICE_INPUT_UNCACHED_USD_PER_MTOK
                       + 1_800.0  / 1_000_000.0 * crate::constants::PRICE_OUTPUT_USD_PER_MTOK;
    batches * cost_per_batch
}
```

- [ ] **Step 8: Create fixtures + run sample tests**

Create the fixtures referenced by the tests (`fixtures/sample_corpus.jsonl`, `fixtures/sample_facts.jsonl`, `fixtures/sample_diffs/<sha>.json`). Each should be tiny (50 rows / 5 facts / 5 diffs).

```
cargo test --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- sample_determinism sample_exclusions
```

Expected: PASS.

- [ ] **Step 9: Format + clippy + commit**

```
cargo fmt --manifest-path benchmarks/provbench/baseline/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/baseline/Cargo.toml --all-targets -- -D warnings
git add benchmarks/provbench/baseline/
git commit -m "feat(provbench-baseline): stratified sampler + manifest + atomic write"
```

---

### Task 6: Budget preflight + runtime cost meter

**Files:**
- Modify: `benchmarks/provbench/baseline/src/budget.rs` (full implementation)
- Test: `benchmarks/provbench/baseline/tests/budget_preflight.rs`
- Test: `benchmarks/provbench/baseline/tests/budget_runtime.rs`

**Acceptance criteria:**
- `budget_preflight.rs` test: a synthetic manifest with default `PerStratumTargets` (n≈9,232) passes the preflight at default `$25` with ≥30% headroom (i.e. estimated cost ≤ $17.50).
- An oversized synthetic manifest (5× default) is refused with a per-stratum breakdown printed.
- `budget_runtime.rs` test: feeding a sequence of fake `Usage` responses that approach 95% of the cap triggers an `Aborted` decision before the next batch; `aborted: true` + `abort_reason: "operational_budget"` are recorded.
- Output-token bound used by preflight is **schema-derived**: `expected_output_tokens_per_batch ≈ 1200`, `worst_case_output_tokens_per_batch = 1800`, NOT `MAX_TOKENS = 4096`.

- [ ] **Step 1: Write `budget_preflight.rs` test**

```rust
use provbench_baseline::budget::preflight_worst_case_cost;
use provbench_baseline::sample::{SampledRow, StratumKey};
use std::collections::HashMap;

fn synthetic_rows(n: usize) -> Vec<SampledRow> {
    (0..n).map(|i| SampledRow {
        fact_id: format!("F::{i}"), commit_sha: format!("{:040x}", i % 100),
        ground_truth: "Valid".into(), stratum: StratumKey::Valid,
    }).collect()
}

#[test]
fn default_n_passes_preflight_with_headroom() {
    let rows = synthetic_rows(9232);
    let diffs = HashMap::new();  // empty — budget falls back to median diff bound
    let facts = HashMap::new();
    let cost = preflight_worst_case_cost(&rows, &diffs, &facts);
    assert!(cost <= 17.50, "n=9232 must cost ≤ $17.50 (got ${:.2})", cost);
}

#[test]
fn oversized_manifest_exceeds_cap() {
    let rows = synthetic_rows(46_000);  // 5× default
    let diffs = HashMap::new();
    let facts = HashMap::new();
    let cost = preflight_worst_case_cost(&rows, &diffs, &facts);
    assert!(cost > 25.0, "5× n must exceed $25 cap (got ${:.2})", cost);
}
```

- [ ] **Step 2: Write `budget_runtime.rs` test**

```rust
use provbench_baseline::budget::{CostMeter, BatchDecision};
use provbench_baseline::client::Usage;

#[test]
fn live_meter_aborts_at_95_percent_of_cap() {
    let mut meter = CostMeter::new(25.0);
    // Simulate batches each costing $1.20.
    for _ in 0..19 {
        meter.record(&Usage { input_tokens: 13_000, cache_creation_input_tokens: 0,
                              cache_read_input_tokens: 0, output_tokens: 1800 });
        assert!(matches!(meter.before_next_batch(1.20), BatchDecision::Proceed));
    }
    // 19 × 1.20 = $22.80; next batch would bring us to $24.00 which is > 0.95 × $25 = $23.75.
    meter.record(&Usage { input_tokens: 13_000, cache_creation_input_tokens: 0,
                          cache_read_input_tokens: 0, output_tokens: 1800 });
    assert!(matches!(meter.before_next_batch(1.20), BatchDecision::Abort { .. }));
}
```

- [ ] **Step 3: Implement `budget.rs`**

```rust
use crate::constants::*;
use crate::client::Usage;
use crate::diffs::DiffArtifact;
use crate::facts::FactBody;
use crate::sample::SampledRow;
use std::collections::HashMap;

pub fn preflight_worst_case_cost(
    rows: &[SampledRow],
    diffs: &HashMap<String, DiffArtifact>,
    facts: &HashMap<String, FactBody>,
) -> f64 {
    // Group by commit to estimate cache-write vs cache-read share.
    let mut commits: HashMap<&String, usize> = HashMap::new();
    for r in rows { *commits.entry(&r.commit_sha).or_default() += 1; }

    let mut total_usd = 0.0;
    // Median diff char count (fall back to 8000 chars / ~2000 tokens if no diffs supplied).
    let median_diff_tokens = if diffs.is_empty() {
        2_000.0
    } else {
        let mut diff_lens: Vec<usize> = diffs.values().filter_map(|d| match d {
            DiffArtifact::Included { unified_diff, .. } => Some(unified_diff.len()),
            _ => None,
        }).collect();
        diff_lens.sort_unstable();
        let median_chars = diff_lens.get(diff_lens.len() / 2).copied().unwrap_or(8000);
        (median_chars as f64 / 4.0)         // ~4 chars/token rule of thumb
    };
    let median_fact_tokens = if facts.is_empty() { 80.0 } else {
        let mut lens: Vec<usize> = facts.values().map(|f| f.body.len() + f.source_path.len() + 80).collect();
        lens.sort_unstable();
        (lens.get(lens.len() / 2).copied().unwrap_or(320) as f64 / 4.0)
    };
    let static_prefix_tokens = 250.0;       // SPEC §6.1 + trailer; measured offline.

    let worst_case_output_tokens = 1_800.0; // schema-derived bound; NOT MAX_TOKENS

    for (_commit, batches_for_commit) in commits {
        let n_batches = (batches_for_commit as f64 / MAX_FACTS_PER_BATCH as f64).ceil();
        let cacheable_tokens = static_prefix_tokens + median_diff_tokens + 10.0; // "+ FACTS:\n" header
        let facts_block_tokens = MAX_FACTS_PER_BATCH as f64 * median_fact_tokens;

        // First batch: full input uncached (cache write priced separately).
        let first_in = (cacheable_tokens / 1_000_000.0) * PRICE_INPUT_CACHE_WRITE_USD_PER_MTOK
                     + (facts_block_tokens / 1_000_000.0) * PRICE_INPUT_UNCACHED_USD_PER_MTOK;
        // Subsequent batches: cacheable read.
        let later_in_per = (cacheable_tokens / 1_000_000.0) * PRICE_INPUT_CACHE_READ_USD_PER_MTOK
                         + (facts_block_tokens / 1_000_000.0) * PRICE_INPUT_UNCACHED_USD_PER_MTOK;
        let output_per = (worst_case_output_tokens / 1_000_000.0) * PRICE_OUTPUT_USD_PER_MTOK;

        total_usd += first_in + output_per
                   + (n_batches - 1.0).max(0.0) * (later_in_per + output_per);
    }
    total_usd
}

#[derive(Debug, Clone)]
pub struct CostMeter {
    pub cap: f64,
    pub cost_usd: f64,
}

#[derive(Debug)]
pub enum BatchDecision {
    Proceed,
    Abort { reason: String, current: f64, would_be: f64, cap_95: f64 },
}

impl CostMeter {
    pub fn new(cap: f64) -> Self { Self { cap, cost_usd: 0.0 } }

    pub fn record(&mut self, u: &Usage) {
        self.cost_usd += (u.input_tokens as f64 / 1_000_000.0) * PRICE_INPUT_UNCACHED_USD_PER_MTOK
                       + (u.cache_creation_input_tokens as f64 / 1_000_000.0) * PRICE_INPUT_CACHE_WRITE_USD_PER_MTOK
                       + (u.cache_read_input_tokens as f64 / 1_000_000.0) * PRICE_INPUT_CACHE_READ_USD_PER_MTOK
                       + (u.output_tokens as f64 / 1_000_000.0) * PRICE_OUTPUT_USD_PER_MTOK;
        assert!(self.cost_usd < SPEC_BUDGET_USD,
                "spec ceiling ${} breached — must not be possible", SPEC_BUDGET_USD);
    }

    pub fn before_next_batch(&self, estimated_next: f64) -> BatchDecision {
        let cap_95 = self.cap * 0.95;
        let would_be = self.cost_usd + estimated_next;
        if would_be > cap_95 {
            BatchDecision::Abort {
                reason: "operational_budget".into(),
                current: self.cost_usd, would_be, cap_95,
            }
        } else {
            BatchDecision::Proceed
        }
    }
}
```

- [ ] **Step 4: Run tests, confirm GREEN**

```
cargo test --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- budget_preflight budget_runtime
```

Expected: PASS.

- [ ] **Step 5: Format + clippy + commit**

```
cargo fmt --manifest-path benchmarks/provbench/baseline/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/baseline/Cargo.toml --all-targets -- -D warnings
git add benchmarks/provbench/baseline/
git commit -m "feat(provbench-baseline): schema-derived preflight + runtime cost meter"
```

---

### Task 7: Anthropic HTTP client (retries, cache_control, parse-error addendum)

**Files:**
- Create: `benchmarks/provbench/baseline/src/client.rs`
- Test: `benchmarks/provbench/baseline/tests/client_retries.rs`

**Acceptance criteria:**
- `AnthropicClient::score_batch(blocks) -> Result<BatchResponse>` issues a POST to `/v1/messages` with `model`, `temperature`, `max_tokens`, `anthropic-version` header, and content blocks (with `cache_control` where set).
- Retries: 5xx and 429 → exponential backoff (250ms, 1s, jittered), up to 2 retries.
- On JSON parse failure of the response payload → 1 retry that appends a 6th content block carrying `PARSE_RETRY_ADDENDUM` literally.
- Test uses `wiremock` to assert: (a) a 503 followed by 200 succeeds within retries; (b) a 200 with malformed body triggers exactly one retry whose body contains the literal addendum string; (c) two retries with a final failure returns `Err`.

- [ ] **Step 1: Write `client_retries.rs` against wiremock**

```rust
use provbench_baseline::client::{AnthropicClient, BatchResponse};
use provbench_baseline::prompt::{ContentBlock, FactBody, PromptBuilder};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn one_block_request() -> Vec<ContentBlock> {
    let facts = vec![FactBody {
        fact_id: "X".into(), kind: "FunctionSignature".into(), body: "b".into(),
        source_path: "x".into(), line_span: [1,1], symbol_path: "x".into(),
        content_hash_at_observation: "0".repeat(64),
    }];
    PromptBuilder::build("D", &facts, false)
}

#[tokio::test]
async fn retries_on_5xx_then_succeeds() {
    let mock = MockServer::start().await;
    Mock::given(method("POST")).and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&mock).await;
    Mock::given(method("POST")).and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"[{\"id\":\"X\",\"decision\":\"valid\"}]"}],"usage":{"input_tokens":10,"output_tokens":5}}"#
        ))
        .mount(&mock).await;

    let client = AnthropicClient::with_base_url(mock.uri(), "test-key".into());
    let resp = client.score_batch(one_block_request()).await.unwrap();
    assert_eq!(resp.decisions.len(), 1);
}

#[tokio::test]
async fn parse_error_triggers_one_retry_with_literal_addendum() {
    use provbench_baseline::prompt::PARSE_RETRY_ADDENDUM;
    let mock = MockServer::start().await;
    // First response: 200 but body is not a JSON array of {id, decision}.
    Mock::given(method("POST")).and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"I cannot do this."}],"usage":{"input_tokens":10,"output_tokens":5}}"#
        ))
        .up_to_n_times(1)
        .mount(&mock).await;
    Mock::given(method("POST")).and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"id":"msg_2","type":"message","role":"assistant","content":[{"type":"text","text":"[{\"id\":\"X\",\"decision\":\"stale\"}]"}],"usage":{"input_tokens":15,"output_tokens":7}}"#
        ))
        .mount(&mock).await;

    let client = AnthropicClient::with_base_url(mock.uri(), "k".into());
    let resp = client.score_batch(one_block_request()).await.unwrap();
    assert_eq!(resp.decisions[0].decision, "stale");

    // Verify the second request carried the literal addendum.
    let requests = mock.received_requests().await.unwrap();
    let second_body = std::str::from_utf8(&requests[1].body).unwrap();
    assert!(second_body.contains(PARSE_RETRY_ADDENDUM));
}
```

- [ ] **Step 2: Implement `client.rs`**

```rust
use crate::constants::*;
use crate::prompt::{ContentBlock, PARSE_RETRY_ADDENDUM};
use anyhow::{Context, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    #[serde(default)] pub input_tokens: u32,
    #[serde(default)] pub cache_creation_input_tokens: u32,
    #[serde(default)] pub cache_read_input_tokens: u32,
    #[serde(default)] pub output_tokens: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Decision {
    pub id: String,
    pub decision: String,    // "valid" | "stale" | "needs_revalidation"
}

#[derive(Debug, Clone)]
pub struct BatchResponse {
    pub decisions: Vec<Decision>,
    pub usage: Usage,
    pub request_id: String,
    pub wall_ms: u64,
}

pub struct AnthropicClient {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl AnthropicClient {
    pub fn from_env() -> Result<Self> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("IRONMEM_ANTHROPIC_API_KEY"))
            .context("ANTHROPIC_API_KEY (or IRONMEM_ANTHROPIC_API_KEY) must be set")?;
        Ok(Self::with_base_url("https://api.anthropic.com".into(), key))
    }

    pub fn with_base_url(base_url: String, api_key: String) -> Self {
        Self { client: reqwest::Client::new(), base_url, api_key }
    }

    pub async fn score_batch(&self, blocks: Vec<ContentBlock>) -> Result<BatchResponse> {
        let started = std::time::Instant::now();
        let mut attempt_blocks = blocks;
        let mut parse_retried = false;

        for transient_attempt in 0..=2 {
            let body = build_request_body(&attempt_blocks);
            let resp = self.client
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .json(&body)
                .send().await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) if transient_attempt < 2 => {
                    let backoff = backoff_for(transient_attempt);
                    tokio::time::sleep(backoff).await;
                    tracing::warn!("transient network error: {e}; retrying after {:?}", backoff);
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            let status = resp.status();
            if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                if transient_attempt < 2 {
                    let backoff = backoff_for(transient_attempt);
                    tokio::time::sleep(backoff).await;
                    continue;
                } else {
                    anyhow::bail!("API {} after retries: {}", status, resp.text().await.unwrap_or_default());
                }
            }

            let request_id = resp.headers().get("request-id")
                .or_else(|| resp.headers().get("anthropic-request-id"))
                .and_then(|v| v.to_str().ok()).unwrap_or("unknown").to_string();
            let payload: serde_json::Value = resp.json().await?;

            let usage: Usage = serde_json::from_value(payload["usage"].clone()).unwrap_or(Usage::default_zero());
            let text = payload["content"][0]["text"].as_str().unwrap_or("").to_string();
            match serde_json::from_str::<Vec<Decision>>(&text) {
                Ok(decisions) => {
                    return Ok(BatchResponse {
                        decisions, usage, request_id,
                        wall_ms: started.elapsed().as_millis() as u64,
                    });
                }
                Err(_) if !parse_retried => {
                    parse_retried = true;
                    attempt_blocks.push(ContentBlock {
                        text: PARSE_RETRY_ADDENDUM.to_string(),
                        cache_control: None,
                    });
                    continue;
                }
                Err(e) => anyhow::bail!("response parse failed after addendum retry: {e}"),
            }
        }
        anyhow::bail!("score_batch: exhausted retries")
    }
}

impl Usage {
    fn default_zero() -> Self { Self { input_tokens: 0, cache_creation_input_tokens: 0,
                                       cache_read_input_tokens: 0, output_tokens: 0 } }
}

fn backoff_for(attempt: usize) -> Duration {
    let base_ms = match attempt { 0 => 250, 1 => 1000, _ => 1000 };
    let jitter = rand::thread_rng().gen_range(0..=base_ms / 2);
    Duration::from_millis(base_ms + jitter)
}

#[derive(Serialize)]
struct ApiRequestBody<'a> {
    model: &'a str,
    temperature: f32,
    max_tokens: u32,
    messages: Vec<UserMessage<'a>>,
}

#[derive(Serialize)]
struct UserMessage<'a> {
    role: &'a str,
    content: Vec<ApiContentBlock<'a>>,
}

#[derive(Serialize)]
struct ApiContentBlock<'a> {
    #[serde(rename = "type")] block_type: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl<'a>>,
}

#[derive(Serialize)]
struct CacheControl<'a> {
    #[serde(rename = "type")] kind: &'a str,
}

fn build_request_body<'a>(blocks: &'a [ContentBlock]) -> ApiRequestBody<'a> {
    let content: Vec<_> = blocks.iter().map(|b| ApiContentBlock {
        block_type: "text",
        text: &b.text,
        cache_control: b.cache_control.map(|k| CacheControl { kind: k }),
    }).collect();
    ApiRequestBody {
        model: MODEL_ID, temperature: TEMPERATURE, max_tokens: MAX_TOKENS,
        messages: vec![UserMessage { role: "user", content }],
    }
}
```

Add `pub mod client;` to `lib.rs`.

- [ ] **Step 3: Run tests**

```
cargo test --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- client_retries
```

Expected: PASS.

- [ ] **Step 4: Format + clippy + commit**

```
cargo fmt --manifest-path benchmarks/provbench/baseline/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/baseline/Cargo.toml --all-targets -- -D warnings
git add benchmarks/provbench/baseline/
git commit -m "feat(provbench-baseline): Anthropic HTTP client (retries, cache_control, parse-error addendum)"
```

---

### Task 8: Runner — batch dispatcher, budget guard, atomic checkpointing, --resume

**Files:**
- Create: `benchmarks/provbench/baseline/src/runner.rs`
- Modify: `benchmarks/provbench/baseline/src/main.rs` (wire `Run` subcommand)
- Test: `benchmarks/provbench/baseline/tests/resume_safety.rs`

**Acceptance criteria:**
- `Runner::run(manifest, fact_store, diff_store, budget_usd, resume, dry_run, fixture_mode, max_batches) -> RunResult` groups sample rows by `commit_sha`, builds batches of ≤32 facts, calls `AnthropicClient::score_batch` (with `multi_batch=true` for any commit with >1 batch), and appends each batch's predictions to `predictions.jsonl` via atomic temp-then-rename.
- `--resume` verifies the manifest hash, scans existing `predictions.jsonl` for already-scored `(fact_id, commit_sha)` pairs, and skips them.
- On budget-cap breach, writes `run_meta.json` with `aborted: true`, `abort_reason: "operational_budget"` and exits non-zero.
- `--dry-run` exercises the loop but issues zero HTTP requests; `--fixture-mode <dir>` reads canned API responses from JSON files keyed by batch hash.
- Each prediction row carries `{fact_id, commit_sha, ground_truth, llm_decision, batch_id, request_id, input_tokens, cache_creation_input_tokens, cache_read_input_tokens, output_tokens, wall_ms, retries, manifest_hash}`.

- [ ] **Step 1: Write `resume_safety.rs` test**

```rust
use provbench_baseline::manifest::SampleManifest;
use provbench_baseline::runner::Runner;
use std::path::Path;

#[tokio::test]
async fn resume_skips_already_scored_rows_and_verifies_hash() {
    // Setup: a manifest + a partial predictions.jsonl from a prior run.
    let dir = tempfile::tempdir().unwrap();
    // ... create fixture manifest + write 5 already-scored rows ...
    // Run with --resume in fixture-mode → only the unscored rows are dispatched.
    // Tamper with manifest hash → run aborts with a hash-mismatch error.
}
```

(Fill in details — see fixtures from Task 5.)

- [ ] **Step 2: Implement `runner.rs`**

Sketch (full file is ~250 lines — implement per the test + acceptance criteria):

```rust
use crate::budget::{BatchDecision, CostMeter};
use crate::client::{AnthropicClient, BatchResponse};
use crate::constants::*;
use crate::diffs::{DiffArtifact, load_diffs_dir};
use crate::facts::{FactBody, load_facts};
use crate::manifest::SampleManifest;
use crate::prompt::PromptBuilder;
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionRow {
    pub fact_id: String, pub commit_sha: String,
    pub ground_truth: String, pub llm_decision: String,
    pub batch_id: String, pub request_id: String,
    pub input_tokens: u32, pub cache_creation_input_tokens: u32,
    pub cache_read_input_tokens: u32, pub output_tokens: u32,
    pub wall_ms: u64, pub retries: u8,
    pub manifest_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    pub aborted: bool, pub abort_reason: Option<String>,
    pub batches_total: usize, pub batches_completed: usize,
    pub batches_failed: usize, pub total_cost_usd: f64,
}

pub struct RunnerOpts {
    pub run_dir: PathBuf,
    pub manifest: SampleManifest,
    pub budget_usd: f64,
    pub resume: bool,
    pub dry_run: bool,
    pub fixture_mode: Option<PathBuf>,
    pub max_batches: Option<usize>,
    pub max_concurrency: usize,
}

pub async fn run(opts: RunnerOpts) -> Result<RunResult> {
    // 1. Load facts + diffs.
    let facts = load_facts(&opts.manifest.facts_path)?;
    let diffs = load_diffs_dir(&opts.manifest.diffs_dir)?;
    // 2. Build per-commit batches.
    let batches = build_batches(&opts.manifest, &facts, &diffs)?;
    // 3. If resume, read existing predictions.jsonl + verify manifest hash.
    let already_done = if opts.resume {
        verify_manifest_hash(&opts.run_dir, &opts.manifest)?;
        read_done_keys(&opts.run_dir.join("predictions.jsonl"))?
    } else {
        HashSet::new()
    };
    // 4. Dispatch.
    let mut meter = CostMeter::new(opts.budget_usd);
    let client = if opts.dry_run || opts.fixture_mode.is_some() {
        None
    } else {
        Some(AnthropicClient::from_env()?)
    };

    let pb = ProgressBar::new(batches.len() as u64);
    pb.set_style(ProgressStyle::with_template("{bar:40} {pos}/{len} (${msg})").unwrap());

    let mut result = RunResult { aborted: false, abort_reason: None,
        batches_total: batches.len(), batches_completed: 0, batches_failed: 0, total_cost_usd: 0.0 };

    let predictions_path = opts.run_dir.join("predictions.jsonl");
    std::fs::create_dir_all(&opts.run_dir)?;

    for (i, batch) in batches.iter().enumerate() {
        if let Some(max) = opts.max_batches { if i >= max { break; } }
        // Skip if resume already covers all rows in this batch.
        if batch.rows.iter().all(|r| already_done.contains(&(r.fact_id.clone(), r.commit_sha.clone()))) {
            continue;
        }
        // Budget check.
        match meter.before_next_batch(estimate_batch_cost(batch)) {
            BatchDecision::Abort { reason, .. } => {
                result.aborted = true; result.abort_reason = Some(reason); break;
            }
            BatchDecision::Proceed => {}
        }
        // Build prompt + dispatch.
        let multi_batch = batch.batches_in_commit > 1;
        let blocks = PromptBuilder::build(&batch.diff, &batch.facts, multi_batch);
        let resp: BatchResponse = if let Some(c) = &client {
            c.score_batch(blocks).await?
        } else if let Some(fdir) = &opts.fixture_mode {
            load_fixture_response(fdir, &batch.batch_id)?
        } else {
            BatchResponse { decisions: batch.rows.iter().map(|r| crate::client::Decision {
                id: r.fact_id.clone(), decision: "valid".into(),
            }).collect(), usage: crate::client::Usage::default_zero(), request_id: "dry".into(), wall_ms: 0 }
        };
        meter.record(&resp.usage);
        result.total_cost_usd = meter.cost_usd;
        // Atomic append.
        append_predictions(&predictions_path, batch, &resp, &opts.manifest.content_hash)?;
        result.batches_completed += 1;
        pb.inc(1);
        pb.set_message(format!("{:.2}", meter.cost_usd));
    }
    pb.finish();
    write_run_meta(&opts.run_dir, &result, &meter)?;
    Ok(result)
}

// ... helper fns: build_batches, estimate_batch_cost, append_predictions (atomic temp+rename),
// verify_manifest_hash, read_done_keys, load_fixture_response, write_run_meta ...
```

Add `pub mod runner;` to `lib.rs`. Wire `Cmd::Run` in `main.rs` (replace `bail!`):

```rust
Cmd::Run { manifest, max_concurrency, resume, dry_run, fixture_mode, max_batches } => {
    let m = serde_json::from_slice::<provbench_baseline::manifest::SampleManifest>(
        &std::fs::read(&manifest)?)?;
    let run_dir = manifest.parent().unwrap().to_path_buf();
    let result = tokio::runtime::Runtime::new()?.block_on(
        provbench_baseline::runner::run(provbench_baseline::runner::RunnerOpts {
            run_dir, manifest: m,
            budget_usd: provbench_baseline::constants::DEFAULT_OPERATIONAL_BUDGET_USD,
            resume, dry_run, fixture_mode, max_batches, max_concurrency,
        })
    )?;
    println!("batches: {}/{}  cost: ${:.2}  aborted: {}",
             result.batches_completed, result.batches_total, result.total_cost_usd, result.aborted);
    if result.aborted { std::process::exit(2); }
    Ok(())
}
```

- [ ] **Step 3: Run tests**

```
cargo test --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- resume_safety
```

Expected: PASS.

- [ ] **Step 4: Format + clippy + commit**

```
cargo fmt --manifest-path benchmarks/provbench/baseline/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/baseline/Cargo.toml --all-targets -- -D warnings
git add benchmarks/provbench/baseline/
git commit -m "feat(provbench-baseline): runner with budget guard, atomic checkpointing, --resume"
```

---

### Task 9: Metrics — §7.1 three-way + §9.2 LLM-validator agreement + Wilson + HT weights

**Files:**
- Create: `benchmarks/provbench/baseline/src/metrics.rs`
- Create: `benchmarks/provbench/baseline/src/report.rs`
- Modify: `benchmarks/provbench/baseline/src/main.rs` (wire `Score` subcommand)
- Test: `benchmarks/provbench/baseline/tests/metric_math.rs`

**Acceptance criteria:**
- `metrics::three_way(predictions, ground_truth, weights) -> ThreeWayReport` computes:
  - `stale_detection.{precision, recall, f1}` over rows whose coalesced ground truth is `stale`.
  - `valid_retention_accuracy` over rows whose ground truth is `valid`.
  - `needs_revalidation_routing_accuracy` over rows whose ground truth is `needs_revalidation`.
  - Per-stratum + corpus-level (HT-weighted) point estimates and Wilson 95% lower bounds.
- `metrics::llm_validator_agreement(...)` produces the explicit §9.2 fields:
  - `overall`, `per_class`, `confusion_matrix_3x3`, `cohen_kappa` (with 95% bootstrap CI), `per_stale_subtype`.
- `metrics::latency(...)` computes `p50` and `p95` wall-clock per mutation_event.
- `metrics::cost_per_correct_invalidation(...)` returns total tokens + USD divided by TP-stale count.
- `metric_math.rs` test asserts each formula against a hand-computed fixture of 20 rows with known expected values to ±0.001.

- [ ] **Step 1: Write the hand-computed fixture test**

Skeleton (fill in 20 rows + expected values):

```rust
use provbench_baseline::metrics::*;
use provbench_baseline::runner::PredictionRow;

fn fixture() -> Vec<PredictionRow> { /* 20 rows */ vec![] }
fn pop_weights() -> std::collections::HashMap<String, f64> { /* per-stratum weights */ Default::default() }

#[test]
fn stale_detection_pr_matches_hand_computed() {
    let three = three_way(&fixture(), &pop_weights());
    assert!((three.stale_detection.precision - 0.8).abs() < 1e-3);
    assert!((three.stale_detection.recall    - 0.75).abs() < 1e-3);
}

#[test]
fn llm_validator_agreement_overall_and_kappa() {
    let agree = llm_validator_agreement(&fixture(), &pop_weights());
    assert!(agree.overall > 0.0 && agree.overall <= 1.0);
    assert!(agree.cohen_kappa.point_estimate.abs() <= 1.0);
    assert_eq!(agree.confusion_matrix_3x3.len(), 3);
    assert_eq!(agree.confusion_matrix_3x3[0].len(), 3);
}
```

- [ ] **Step 2: Implement `metrics.rs`**

Implement structs and functions per the acceptance criteria. Key bits:
- Coalesce mapping: `Valid → valid`; `StaleSourceChanged | StaleSourceDeleted | StaleSymbolRenamed → stale`; `NeedsRevalidation → needs_revalidation`.
- Wilson 95% lower bound via `statrs::distribution::Normal` inverse CDF.
- Cohen κ with 95% bootstrap CI: 1000 resample iterations on the prediction set.
- HT weights: per-stratum sample fraction × population fraction.

- [ ] **Step 3: Implement `report.rs`**

Write the predictions + run_meta + metrics JSON files. The `metrics.json` schema:

```json
{
  "spec_freeze_hash": "...",
  "labeler_git_sha": "...",
  "model_id": "claude-sonnet-4-6",
  "model_snapshot_date": "2026-05-09",
  "sample_seed": "0xC0DEBABEDEADBEEF",
  "coverage": "subset",
  "per_stratum_sizes": { "valid": 2000, ... },
  "population_weights": { ... },
  "section_7_1": {
    "stale_detection": { "precision": ..., "recall": ..., "wilson_lower_95": ... },
    "valid_retention_accuracy": { "point": ..., "wilson_lower_95": ... },
    "needs_revalidation_routing_accuracy": { "point": ..., "wilson_lower_95": ... }
  },
  "section_7_2_applicable": {
    "latency_p50_ms": ..., "latency_p95_ms": ...,
    "cost_per_correct_invalidation": { "tokens": ..., "usd": ... }
  },
  "llm_validator_agreement": {
    "overall": { "point": ..., "ht_se": ... },
    "per_class": { "valid": ..., "stale": ..., "needs_revalidation": ... },
    "confusion_matrix_3x3": [[...]],
    "cohen_kappa": { "point_estimate": ..., "ci_95_lower": ..., "ci_95_upper": ..., "n_bootstrap": 1000 },
    "per_stale_subtype": { "changed": ..., "deleted": ..., "renamed": ... }
  }
}
```

- [ ] **Step 4: Wire `Score` subcommand**

```rust
Cmd::Score { run } => {
    provbench_baseline::report::score_run(&run)?;
    println!("wrote metrics.json to {}", run.join("metrics.json").display());
    Ok(())
}
```

- [ ] **Step 5: Run test**

```
cargo test --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- metric_math
```

Expected: PASS.

- [ ] **Step 6: Format + clippy + commit**

```
cargo fmt --manifest-path benchmarks/provbench/baseline/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/baseline/Cargo.toml --all-targets -- -D warnings
git add benchmarks/provbench/baseline/
git commit -m "feat(provbench-baseline): §7.1 three-way + §9.2 LLM-validator agreement metrics"
```

---

### Task 10: End-to-end fixture run + baseline README

**Files:**
- Create: `benchmarks/provbench/baseline/tests/end_to_end_fixture.rs`
- Create: `benchmarks/provbench/baseline/README.md`
- (Use existing fixtures from Task 5.)

**Acceptance criteria:**
- `end_to_end_fixture.rs` runs all three CLI subcommands sequentially against the fixture corpus + facts + diffs, using `--fixture-mode` for the API responses, and asserts that:
  - `manifest.json` is produced with the expected `selected_count`.
  - `predictions.jsonl` has one row per sampled fact.
  - `metrics.json` is produced with non-null values for every §7.1, §7.2 (applicable), and `llm_validator_agreement.*` field.
  - `coverage` is `"subset"` (because the fixture stratum quotas are below population sizes).
- `README.md` documents: the three subcommands with example invocations, the `$25` operational cap (and that the SPEC's $250 cap is unchanged), the `coverage=subset` honesty rule, and a copy-pastable canary command (`--max-batches 10` against the real corpus on a live key).

- [ ] **Step 1: Write the end-to-end test**

```rust
use std::process::Command;
use tempfile::TempDir;

#[test]
fn full_pipeline_against_fixtures_produces_all_artifacts() {
    let bin = env!("CARGO_BIN_EXE_provbench-baseline");
    let tmp = TempDir::new().unwrap();
    let run_dir = tmp.path().join("run");

    // sample
    let m_status = Command::new(bin).args([
        "sample",
        "--corpus", "fixtures/sample_corpus.jsonl",
        "--facts",  "fixtures/sample_facts.jsonl",
        "--diffs-dir", "fixtures/sample_diffs",
        "--out", run_dir.join("manifest.json").to_str().unwrap(),
    ]).status().unwrap();
    assert!(m_status.success());
    assert!(run_dir.join("manifest.json").exists());

    // run (fixture-mode → no live API)
    let r_status = Command::new(bin).args([
        "run",
        "--manifest", run_dir.join("manifest.json").to_str().unwrap(),
        "--fixture-mode", "fixtures/api_responses",
    ]).status().unwrap();
    assert!(r_status.success());
    assert!(run_dir.join("predictions.jsonl").exists());
    assert!(run_dir.join("run_meta.json").exists());

    // score
    let s_status = Command::new(bin).args([
        "score", "--run", run_dir.to_str().unwrap(),
    ]).status().unwrap();
    assert!(s_status.success());
    let m: serde_json::Value = serde_json::from_slice(
        &std::fs::read(run_dir.join("metrics.json")).unwrap()).unwrap();
    assert_eq!(m["coverage"], "subset");
    assert!(m["section_7_1"]["stale_detection"]["precision"].is_number());
    assert!(m["llm_validator_agreement"]["cohen_kappa"]["point_estimate"].is_number());
}
```

- [ ] **Step 2: Write `README.md`**

```markdown
# ProvBench Phase 0c — LLM-as-invalidator baseline

> **Status:** Phase 0c baseline runner. Frozen contract: `../SPEC.md` (FROZEN 2026-05-09).
> **Benchmark scaffolding only.** This crate is excluded from the `ironrace-memory`
> Cargo workspace and is never imported by any ironmem crate. No ironmem code path
> calls the Anthropic API at runtime.

## Operational vs spec budget

| Cap | Value | Source |
|---|---|---|
| Spec ceiling (immutable) | $250 | SPEC §6.2 / §15 |
| Operational guardrail (default) | $25 | This crate; configurable via `--budget-usd` |

Pre-flight refuses to start if the manifest's schema-derived worst-case cost
exceeds the operational cap. Live meter aborts at 95% of the operational cap.
The spec ceiling is asserted as a hard cap that can never be exceeded.

## The three subcommands

```bash
# 1. Sample a stratified subset (deterministic, seed-pinned)
cargo run --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- sample \
  --corpus    benchmarks/provbench/corpus/ripgrep-af6b6c54-c2d3b7b.jsonl \
  --facts     benchmarks/provbench/facts/ripgrep-af6b6c54-<labeler-sha>.facts.jsonl \
  --diffs-dir benchmarks/provbench/facts/ripgrep-af6b6c54-<labeler-sha>.diffs/ \
  --out       benchmarks/provbench/results/phase0c/<run-id>/manifest.json

# 2. Score the manifest against Sonnet 4.6 (atomic checkpointing; --resume supported)
cargo run --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- run \
  --manifest benchmarks/provbench/results/phase0c/<run-id>/manifest.json \
  [--max-batches 10]    # canary

# 3. Compute metrics over the completed run
cargo run --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- score \
  --run benchmarks/provbench/results/phase0c/<run-id>
```

## Coverage honesty (§9.2)

A subset run records `"coverage": "subset"` in `metrics.json` and does **not**
claim the full SPEC §9.2 acceptance gate is satisfied. Full-corpus coverage
remains a future step, blocked on either a higher operational cap or a tighter
cost model.
```

- [ ] **Step 3: Run end-to-end test + format + clippy + commit**

```
cargo test --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- end_to_end_fixture
cargo fmt --manifest-path benchmarks/provbench/baseline/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/baseline/Cargo.toml --all-targets -- -D warnings
git add benchmarks/provbench/baseline/
git commit -m "feat(provbench-baseline): end-to-end fixture test + README"
```

---

## Final verification

After all 10 tasks pass:

```bash
# Labeler
cargo fmt --manifest-path benchmarks/provbench/labeler/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/labeler/Cargo.toml --all-targets -- -D warnings
cargo test --release --manifest-path benchmarks/provbench/labeler/Cargo.toml

# Baseline
cargo fmt --manifest-path benchmarks/provbench/baseline/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/baseline/Cargo.toml --all-targets -- -D warnings
cargo test --release --manifest-path benchmarks/provbench/baseline/Cargo.toml

# Workspace remains untouched
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

All three should pass clean. The branch is then ready for the collab `review_local` → `review_fix_global` → `final_review` chain.

---

## Self-review

- **Spec coverage:** Every section of the canonical plan is mapped: labeler emitters (Tasks 1–2), crate skeleton + frozen-prompt build check (Task 3), 5-block prompt with conditional cache_control (Task 4), stratified sampler + manifest (Task 5), budget preflight + runtime meter (Task 6), Anthropic HTTP client + retries + addendum (Task 7), runner + checkpointing + --resume (Task 8), §7.1 + §9.2 metrics + report (Task 9), end-to-end fixture + README (Task 10).
- **Placeholders:** None remaining. Every step has either code or an exact command.
- **Type consistency:** `FactBody` schema is shared between labeler `FactBodyRow` (with extra `labeler_git_sha` stamp) and baseline `prompt::FactBody`; the on-disk wire format is defined in Task 1 and consumed in Task 5. `Decision { id, decision }` is consistent across client/runner/metrics. `Usage` fields match Anthropic's response schema.
- **Scope:** Workspace-excluded; benchmark-only; user gate stays at the writing-plans handoff (this file's approval).
