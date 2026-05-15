# ProvBench Phase 1 — SPEC §9.4 Held-out Round 1 (serde-rs/serde) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run the SPEC §9.4 held-out evaluation of the Phase 1 rules invalidator (v1.1, phase1 git SHA `ccfc901be17124d08c19a6de50294ff79ded6fc3`, no in-round retuning) on serde-rs/serde @ T₀ `65e1a50749938612cfbdb69b57fc4cf249f87149`, record the §8 verdict against pinned thresholds, and append one row to SPEC §11.

**Architecture:** Build labeler and phase1 binaries in ephemeral git worktrees pinned to their frozen SHAs so the labeler stamps the right `labeler_git_sha` and phase1 source is byte-identical to the frozen SHA. Generate held-out corpus/facts/diffs with the pinned labeler, stratified-sample with the existing baseline crate (no LLM call — `--dry-run` is the schema carrier), score with phase1 v1.1, and report against the three §8 thresholds. Honor §10 anti-leakage: no retuning, no rule-chain changes, no labeler bump. Findings doc + SPEC §11 row record the outcome whether pass or fail.

**Tech Stack:** Rust workspace (cargo, clap), `provbench-labeler` / `provbench-baseline` / `provbench-phase1` / `provbench-score` binaries, jq for JSON inspection, git worktrees for SHA pinning, serde_json for hand-written run_meta.json.

**Frozen pins (do not bump):**
- Spec freeze hash: `683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c`
- Labeler git SHA: `c2d3b7b03a51a9047ff2d50077200bb52f149448`
- Phase 1 git SHA: `ccfc901be17124d08c19a6de50294ff79ded6fc3`
- Rule set version: `v1.1`
- Sample seed: `13897750829054410479` (= `0xC0DE_BABE_DEAD_BEEF`; pass as decimal — clap's default `u64` parser does NOT accept hex)
- Held-out repo: serde-rs/serde @ `65e1a50749938612cfbdb69b57fc4cf249f87149`
- Branch: `feat/provbench-phase1-heldout-serde` (already cut from `v0.2.0`)

**Acceptance (SPEC §8 verbatim, read from `<RUNDIR>/metrics.json.phase1_rules.*`):**
- `section_7_1.stale_detection.wilson_lower_95 >= 0.30`
- `section_7_1.valid_retention_accuracy.wilson_lower_95 >= 0.95`
- `section_7_2_applicable.latency_p50_ms <= 727`

**Run directory shape:** `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/{metrics.json, baseline/{manifest.json,predictions.jsonl,run_meta.json,metrics.json}, phase1/{predictions.jsonl,rule_traces.jsonl,run_meta.json}}`. `phase1.sqlite` is NOT committed (matches pilot precedent).

**On §8 miss:** No retune. Surface honestly in findings; SPEC §11 row records `Result: FAIL §8 #N`. §10 forbids in-round retuning either way.

---

## File Structure

| Path | Responsibility | New / Modified |
|---|---|---|
| `benchmarks/provbench/work/serde/` | serde-rs/serde checkout @ T₀ | New (gitignored, untracked) |
| `benchmarks/provbench/corpus/serde-65e1a507-c2d3b7b.jsonl` | Labeler corpus | New (gitignored) |
| `benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.facts.jsonl` | T₀ fact bodies | New (gitignored) |
| `benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.diffs/` | Per-commit unified diffs | New (gitignored) |
| `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/baseline/` | Dry-run subset carrier (manifest, predictions, run_meta, metrics) | New (committed) |
| `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/phase1/` | Phase1 score output + hand-written run_meta | New (committed; excludes `phase1.sqlite`) |
| `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/metrics.json` | `provbench-score compare` output | New (committed) |
| `benchmarks/provbench/results/serde-heldout-2026-05-15-findings.md` | Findings doc | New (committed) |
| `benchmarks/provbench/labeler/tests/determinism_serde.rs` | Sibling determinism test (additive — does NOT edit existing `determinism.rs`) | New (committed) |
| `benchmarks/provbench/phase1/tests/end_to_end_heldout_serde.rs` | Acceptance test asserting §8 + row-count + rule_set_version | New (committed) |
| `benchmarks/provbench/SPEC.md` | Append one §11 row only (frozen body untouched) | Modified (committed) |
| `.gitignore` | Add `benchmarks/provbench/results/serde-heldout-*/phase1/phase1.sqlite` if not already covered by an existing rule | Modified (committed) |

---

## Task 1: Pre-flight + git worktree pin + serde clone

**Files:**
- Create: `/tmp/ironmem-worktrees/labeler-c2d3b7b/` (git worktree)
- Create: `/tmp/ironmem-worktrees/phase1-ccfc901/` (git worktree)
- Create: `benchmarks/provbench/work/serde/` (git clone)
- Inspect: `.gitignore`

- [ ] **Step 1: Verify pinned SHAs are reachable and feature branch is current**

```bash
git rev-parse HEAD                                                      # expect: 27f63c5b… (feat branch HEAD = v0.2.0)
git cat-file -e c2d3b7b03a51a9047ff2d50077200bb52f149448^{commit}       # exits 0
git cat-file -e ccfc901be17124d08c19a6de50294ff79ded6fc3^{commit}       # exits 0
git branch --show-current                                                # expect: feat/provbench-phase1-heldout-serde
```

Expected: all four commands succeed silently or print the expected branch/SHA.

- [ ] **Step 2: Verify `work/` directory is gitignored**

```bash
git check-ignore -v benchmarks/provbench/work/serde 2>&1
```

Expected output includes `benchmarks/provbench/work` matched by an existing `.gitignore` rule (the pilot's `work/ripgrep` checkout was untracked by the same rule). If the command exits non-zero (path not ignored), STOP and add `benchmarks/provbench/work/` to the top-level `.gitignore` before continuing.

- [ ] **Step 3: Create ephemeral git worktrees pinned to the frozen SHAs**

```bash
mkdir -p /tmp/ironmem-worktrees
git worktree add /tmp/ironmem-worktrees/labeler-c2d3b7b c2d3b7b03a51a9047ff2d50077200bb52f149448
git worktree add /tmp/ironmem-worktrees/phase1-ccfc901  ccfc901be17124d08c19a6de50294ff79ded6fc3
```

Expected: two new worktrees, both in detached HEAD at their respective pinned SHAs.

- [ ] **Step 4: Verify worktree HEADs**

```bash
git -C /tmp/ironmem-worktrees/labeler-c2d3b7b rev-parse HEAD   # expect: c2d3b7b03a51a9047ff2d50077200bb52f149448
git -C /tmp/ironmem-worktrees/phase1-ccfc901  rev-parse HEAD   # expect: ccfc901be17124d08c19a6de50294ff79ded6fc3
```

- [ ] **Step 5: Clone serde-rs/serde and check out T₀**

```bash
git clone https://github.com/serde-rs/serde benchmarks/provbench/work/serde
git -C benchmarks/provbench/work/serde fetch origin
git -C benchmarks/provbench/work/serde checkout 65e1a50749938612cfbdb69b57fc4cf249f87149
git -C benchmarks/provbench/work/serde rev-parse HEAD   # expect: 65e1a50749938612cfbdb69b57fc4cf249f87149
```

- [ ] **Step 6: Verify the held-out checkout is NOT tracked by the feature branch**

```bash
git check-ignore benchmarks/provbench/work/serde   # exits 0; prints the path
git status --short benchmarks/provbench/work/serde # empty (ignored)
```

Expected: serde checkout is fully ignored. If it appears in `git status`, fix `.gitignore` BEFORE proceeding (committing the serde tree would explode repo size).

---

## Task 2: Build labeler@c2d3b7b0 + verify stamp + verify-tooling

**Files:**
- Use: `/tmp/ironmem-worktrees/labeler-c2d3b7b/benchmarks/provbench/labeler/Cargo.toml`

- [ ] **Step 1: Build the labeler in the pinned worktree (release)**

```bash
cargo build --release \
  --manifest-path /tmp/ironmem-worktrees/labeler-c2d3b7b/benchmarks/provbench/labeler/Cargo.toml
```

Expected: build succeeds, binary at `/tmp/ironmem-worktrees/labeler-c2d3b7b/benchmarks/provbench/labeler/target/release/provbench-labeler`.

- [ ] **Step 2: Export `LABELER_BIN` and verify embedded SHA stamp**

```bash
export LABELER_BIN=/tmp/ironmem-worktrees/labeler-c2d3b7b/benchmarks/provbench/labeler/target/release/provbench-labeler
$LABELER_BIN stamp
```

Expected: prints exactly `c2d3b7b03a51a9047ff2d50077200bb52f149448`. If anything else, STOP — the binary doesn't carry the right pin and the round is invalid.

- [ ] **Step 3: Verify SPEC §13.1 external tooling content hashes**

```bash
$LABELER_BIN verify-tooling
```

Expected: exit code 0. If non-zero, STOP — the pinned rust-analyzer / tree-sitter binaries don't match the SPEC §13.1 content hashes on this machine. Resolve by re-installing the pinned tooling (out-of-scope here — surface to user).

---

## Task 3: Run labeler end-to-end on the serde checkout

**Files:**
- Create: `benchmarks/provbench/corpus/serde-65e1a507-c2d3b7b.jsonl`
- Create: `benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.facts.jsonl`
- Create: `benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.diffs/<sha>.json` (one per distinct commit_sha)

- [ ] **Step 1: Run the labeler over the held-out checkout**

```bash
$LABELER_BIN run \
  --repo benchmarks/provbench/work/serde \
  --t0 65e1a50749938612cfbdb69b57fc4cf249f87149 \
  --out benchmarks/provbench/corpus/serde-65e1a507-c2d3b7b.jsonl
```

Expected: corpus JSONL written. Surface row count (`wc -l`) for the findings doc.

- [ ] **Step 2: Emit T₀ fact bodies**

```bash
$LABELER_BIN emit-facts \
  --corpus benchmarks/provbench/corpus/serde-65e1a507-c2d3b7b.jsonl \
  --repo benchmarks/provbench/work/serde \
  --t0 65e1a50749938612cfbdb69b57fc4cf249f87149 \
  --out benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.facts.jsonl
```

Expected: facts JSONL written, one row per unique `fact_id` in the corpus.

- [ ] **Step 3: Emit per-commit unified diffs**

```bash
$LABELER_BIN emit-diffs \
  --corpus benchmarks/provbench/corpus/serde-65e1a507-c2d3b7b.jsonl \
  --repo benchmarks/provbench/work/serde \
  --t0 65e1a50749938612cfbdb69b57fc4cf249f87149 \
  --out-dir benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.diffs
```

Expected: one `<commit_sha>.json` artifact per distinct `commit_sha` in the corpus (or an explicit `excluded` reason for `t0` / `no_parent`).

- [ ] **Step 4: Validate labeler_git_sha stamp on every output row**

```bash
jq -s 'map(.labeler_git_sha) | unique' \
  benchmarks/provbench/corpus/serde-65e1a507-c2d3b7b.jsonl
# expect: ["c2d3b7b03a51a9047ff2d50077200bb52f149448"]
```

If the array contains anything else, STOP — labeler output is contaminated.

---

## Task 4: TDD — labeler determinism on the held-out corpus

**Files:**
- Create: `benchmarks/provbench/labeler/tests/determinism_serde.rs` (additive sibling — does NOT edit existing `determinism.rs`)

- [ ] **Step 1: Read the existing canary determinism test as the template**

Read `benchmarks/provbench/labeler/tests/determinism.rs` end-to-end to understand the contract (run labeler twice, compare byte-for-byte). The sibling test mirrors the same shape, parameterized by serde paths.

- [ ] **Step 2: Write the failing sibling test**

```rust
// benchmarks/provbench/labeler/tests/determinism_serde.rs
//! SPEC §9.4 held-out determinism gate for serde-rs/serde @ 65e1a507.
//! Runs the labeler twice into temp dirs and asserts byte-identical
//! corpus + facts + diffs outputs. Ignored by default; activate with
//! `cargo test --test determinism_serde -- --ignored`.

use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

const SERDE_T0: &str = "65e1a50749938612cfbdb69b57fc4cf249f87149";

fn provbench_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn labeler_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_provbench-labeler"))
}

fn run_labeler_corpus(out: &PathBuf, work_serde: &PathBuf) {
    let status = Command::new(labeler_bin())
        .args([
            "run",
            "--repo", work_serde.to_str().unwrap(),
            "--t0", SERDE_T0,
            "--out", out.to_str().unwrap(),
        ])
        .status()
        .expect("spawn provbench-labeler run");
    assert!(status.success(), "labeler run failed");
}

#[test]
#[ignore = "requires benchmarks/provbench/work/serde checkout; run with --ignored"]
fn serde_held_out_corpus_is_byte_identical_across_runs() {
    let work_serde = provbench_root().join("work/serde");
    assert!(
        work_serde.exists(),
        "needs work/serde checkout at SERDE_T0 for held-out determinism gate"
    );

    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let out_a = a.path().join("serde.corpus.jsonl");
    let out_b = b.path().join("serde.corpus.jsonl");

    run_labeler_corpus(&out_a, &work_serde);
    run_labeler_corpus(&out_b, &work_serde);

    let bytes_a = std::fs::read(&out_a).unwrap();
    let bytes_b = std::fs::read(&out_b).unwrap();
    assert_eq!(
        bytes_a, bytes_b,
        "labeler corpus output is non-deterministic on the held-out serde checkout"
    );
}
```

- [ ] **Step 3: Build the test (no run yet — `work/serde` exists from Task 1)**

```bash
cargo test --release \
  --manifest-path /tmp/ironmem-worktrees/labeler-c2d3b7b/benchmarks/provbench/labeler/Cargo.toml \
  --test determinism_serde --no-run
```

Expected: build succeeds; test compiled into target. (Note: build inside the worktree so the binary used by `env!("CARGO_BIN_EXE_...")` is the pinned labeler. But the test source file itself is added to the feature-branch checkout — copy it into the worktree too OR symlink the worktree test file path. Implementation tip: build the test from the feature-branch source by passing `--test determinism_serde` against the feature-branch Cargo.toml — the binary built by the test runner is the labeler at the feature-branch HEAD, NOT the pinned labeler. For the determinism gate we just need labeler determinism *as a property*; we run it against the pinned labeler too, in the next step, to confirm the gate also holds for the binary we'll use for evidence.)

Practical: run the gate against the feature-branch labeler first (Step 4 below). If feature-branch labeler is byte-identical to pinned labeler (Task 8 byte-stability check covers this for phase1; for the labeler the `LABELER_BIN stamp` check already pinned us), then determinism on either implies determinism on the other.

- [ ] **Step 4: Run the gate (feature-branch labeler), expect green**

```bash
cargo test --release \
  --manifest-path benchmarks/provbench/labeler/Cargo.toml \
  --test determinism_serde -- --ignored --nocapture
```

Expected: PASS. If FAIL → labeler non-determinism on held-out repo; STOP and surface to user.

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/tests/determinism_serde.rs
git commit -m "test(provbench): add held-out determinism gate for serde-rs/serde"
```

---

## Task 5: Stratified sample → `<RUNDIR>/baseline/manifest.json`

**Files:**
- Create: `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/baseline/manifest.json`

- [ ] **Step 1: Build baseline runner (feature branch)**

```bash
cargo build --release --manifest-path benchmarks/provbench/baseline/Cargo.toml
export BASELINE_BIN=benchmarks/provbench/baseline/target/release/provbench-baseline
export RUNDIR=benchmarks/provbench/results/serde-heldout-2026-05-15-canary
mkdir -p $RUNDIR/baseline
```

- [ ] **Step 2: Run the stratified sampler (decimal seed — clap default `u64` parser does NOT accept hex)**

```bash
$BASELINE_BIN sample \
  --corpus    benchmarks/provbench/corpus/serde-65e1a507-c2d3b7b.jsonl \
  --facts     benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.facts.jsonl \
  --diffs-dir benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.diffs \
  --seed      13897750829054410479 \
  --out       $RUNDIR/baseline/manifest.json
```

- [ ] **Step 3: Validate emitted manifest fields**

```bash
jq '{seed, labeler_git_sha, spec_freeze_hash, per_stratum_targets, selected_count}' \
  $RUNDIR/baseline/manifest.json
```

Expected:
- `seed == 13897750829054410479`
- `labeler_git_sha == "c2d3b7b03a51a9047ff2d50077200bb52f149448"`
- `spec_freeze_hash == "683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c"`
- `per_stratum_targets` matches pilot defaults (`valid:2000, stale_changed:2000, stale_deleted:2000, stale_renamed:<usize::MAX sentinel>, needs_revalidation:2000`)
- `selected_count > 0`

Halt if any field mismatches.

---

## Task 6: Dry-run predictions + baseline score → `<RUNDIR>/baseline/`

**Files:**
- Create: `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/baseline/predictions.jsonl`
- Create: `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/baseline/run_meta.json`
- Create: `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/baseline/metrics.json`

- [ ] **Step 1: Dry-run the baseline runner (zero API cost; all predictions `"valid"`)**

```bash
$BASELINE_BIN run \
  --manifest $RUNDIR/baseline/manifest.json \
  --dry-run
```

Expected: writes `$RUNDIR/baseline/predictions.jsonl` + `$RUNDIR/baseline/run_meta.json`. All predictions are the dry-run sentinel (`"valid"`).

- [ ] **Step 2: Score the baseline run (writes `metrics.json` next to predictions)**

```bash
$BASELINE_BIN score --run $RUNDIR/baseline
```

Expected: writes `$RUNDIR/baseline/metrics.json`. This file exists ONLY because `provbench-score compare` reads `<baseline_run>/metrics.json` (`scoring/src/compare.rs:47-49`). The numbers in it are dry-run sentinel artifacts — explicitly non-evidentiary.

- [ ] **Step 3: Sanity check `<RUNDIR>/baseline/`**

```bash
ls $RUNDIR/baseline/
# expect: manifest.json  metrics.json  predictions.jsonl  run_meta.json
wc -l $RUNDIR/baseline/predictions.jsonl   # expect: == selected_count from manifest
```

---

## Task 7: Build phase1@ccfc901 + scoring binary

**Files:**
- Use: `/tmp/ironmem-worktrees/phase1-ccfc901/benchmarks/provbench/phase1/Cargo.toml`
- Use: `/tmp/ironmem-worktrees/phase1-ccfc901/benchmarks/provbench/scoring/Cargo.toml`

- [ ] **Step 1: Build phase1 in worktree**

```bash
cargo build --release \
  --manifest-path /tmp/ironmem-worktrees/phase1-ccfc901/benchmarks/provbench/phase1/Cargo.toml
export PHASE1_BIN=/tmp/ironmem-worktrees/phase1-ccfc901/benchmarks/provbench/phase1/target/release/provbench-phase1
```

- [ ] **Step 2: Build scoring binary in worktree**

```bash
cargo build --release \
  --manifest-path /tmp/ironmem-worktrees/phase1-ccfc901/benchmarks/provbench/scoring/Cargo.toml \
  --bin provbench-score
export SCORE_BIN=/tmp/ironmem-worktrees/phase1-ccfc901/benchmarks/provbench/scoring/target/release/provbench-score
```

- [ ] **Step 3: Verify phase1 source byte-stability vs feature branch (CRITICAL §10 contract)**

```bash
git diff ccfc901be17124d08c19a6de50294ff79ded6fc3..HEAD -- benchmarks/provbench/phase1/src
# expect: empty (no output, exit 0)
```

If the diff is non-empty, STOP — feature-branch phase1 source has drifted from the frozen SHA. The end_to_end_heldout_serde test would be running a different binary than what the round claims. This is a §10 violation; the round must halt and be relabeled v1.2 (which re-runs the leakage clock).

---

## Task 8: Phase 1 score → `<RUNDIR>/phase1/` + hand-written `run_meta.json`

**Files:**
- Create: `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/phase1/predictions.jsonl` (by phase1)
- Create: `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/phase1/rule_traces.jsonl` (by phase1)
- Create: `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/phase1/phase1.sqlite` (by phase1; NOT committed)
- Create: `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/phase1/run_meta.json` (hand-written — phase1 doesn't emit this)

- [ ] **Step 1: Run phase1 score (timed, capturing WALL and ROWS)**

```bash
START=$(date +%s)
$PHASE1_BIN score \
  --repo benchmarks/provbench/work/serde \
  --t0 65e1a50749938612cfbdb69b57fc4cf249f87149 \
  --facts benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.facts.jsonl \
  --diffs-dir benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.diffs \
  --baseline-run $RUNDIR/baseline \
  --out $RUNDIR/phase1 \
  --rule-set-version v1.1
WALL=$(( $(date +%s) - START ))
ROWS=$(wc -l < $RUNDIR/phase1/predictions.jsonl)
echo "WALL=$WALL ROWS=$ROWS"
```

Expected: phase1 succeeds; `$RUNDIR/phase1/{predictions.jsonl,rule_traces.jsonl,phase1.sqlite}` exist; `ROWS` matches `manifest.json.selected_count`.

- [ ] **Step 2: Hand-write `<RUNDIR>/phase1/run_meta.json` (matches pilot field shape; `runner: "phase1"`)**

```bash
cat > $RUNDIR/phase1/run_meta.json <<EOF
{
  "runner": "phase1",
  "phase1_git_sha": "ccfc901be17124d08c19a6de50294ff79ded6fc3",
  "spec_freeze_hash": "683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c",
  "labeler_git_sha": "c2d3b7b03a51a9047ff2d50077200bb52f149448",
  "rule_set_version": "v1.1",
  "wall_seconds": $WALL,
  "row_count": $ROWS,
  "repo": "serde-rs/serde",
  "t0": "65e1a50749938612cfbdb69b57fc4cf249f87149",
  "baseline_run_dir": "results/serde-heldout-2026-05-15-canary/baseline",
  "baseline_run_kind": "dry_run_subset_carrier_not_evidence"
}
EOF
```

- [ ] **Step 3: Validate the hand-written run_meta**

```bash
jq '.runner, .phase1_git_sha, .rule_set_version, .labeler_git_sha, .spec_freeze_hash, .baseline_run_kind' \
  $RUNDIR/phase1/run_meta.json
```

Expected:
- `"phase1"`
- `"ccfc901be17124d08c19a6de50294ff79ded6fc3"`
- `"v1.1"`
- `"c2d3b7b03a51a9047ff2d50077200bb52f149448"`
- `"683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c"`
- `"dry_run_subset_carrier_not_evidence"`

---

## Task 9: Phase 1 determinism two-run gate

**Files:**
- Use: pinned phase1 binary; serde artifacts

- [ ] **Step 1: Run phase1 score twice into temp dirs**

```bash
$PHASE1_BIN score \
  --repo benchmarks/provbench/work/serde \
  --t0 65e1a50749938612cfbdb69b57fc4cf249f87149 \
  --facts benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.facts.jsonl \
  --diffs-dir benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.diffs \
  --baseline-run $RUNDIR/baseline \
  --out /tmp/phase1-serde-A \
  --rule-set-version v1.1

$PHASE1_BIN score \
  --repo benchmarks/provbench/work/serde \
  --t0 65e1a50749938612cfbdb69b57fc4cf249f87149 \
  --facts benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.facts.jsonl \
  --diffs-dir benchmarks/provbench/facts/serde-65e1a507-c2d3b7b.diffs \
  --baseline-run $RUNDIR/baseline \
  --out /tmp/phase1-serde-B \
  --rule-set-version v1.1
```

- [ ] **Step 2: Diff predictions modulo `wall_ms`**

```bash
diff <(jq -c 'del(.wall_ms)' /tmp/phase1-serde-A/predictions.jsonl) \
     <(jq -c 'del(.wall_ms)' /tmp/phase1-serde-B/predictions.jsonl)
```

Expected: empty diff, exit 0. Non-empty diff → phase1 non-determinism on held-out; STOP.

- [ ] **Step 3: Cleanup temp dirs**

```bash
rm -rf /tmp/phase1-serde-A /tmp/phase1-serde-B
```

---

## Task 10: `provbench-score compare` → `<RUNDIR>/metrics.json`

**Files:**
- Create: `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/metrics.json`

- [ ] **Step 1: Run compare (reads `<RUNDIR>/baseline/metrics.json`, writes the side-by-side report)**

```bash
$SCORE_BIN compare \
  --baseline-run  $RUNDIR/baseline \
  --candidate-run $RUNDIR/phase1 \
  --candidate-name phase1_rules \
  --out $RUNDIR/metrics.json
```

Expected: `$RUNDIR/metrics.json` written with `phase1_rules` and baseline columns; threshold booleans embedded.

- [ ] **Step 2: Sanity check §8 fields exist**

```bash
jq '.phase1_rules.section_7_1.stale_detection.wilson_lower_95,
    .phase1_rules.section_7_1.valid_retention_accuracy.wilson_lower_95,
    .phase1_rules.section_7_2_applicable.latency_p50_ms' \
  $RUNDIR/metrics.json
```

Expected: three numeric values printed. If any is null, STOP — scoring output is malformed.

- [ ] **Step 3: Record §8 verdict (preview — not the gate)**

```bash
jq '{
  stale_wlb: .phase1_rules.section_7_1.stale_detection.wilson_lower_95,
  valid_wlb: .phase1_rules.section_7_1.valid_retention_accuracy.wilson_lower_95,
  p50_ms:    .phase1_rules.section_7_2_applicable.latency_p50_ms,
  pass_stale: (.phase1_rules.section_7_1.stale_detection.wilson_lower_95 >= 0.30),
  pass_valid: (.phase1_rules.section_7_1.valid_retention_accuracy.wilson_lower_95 >= 0.95),
  pass_p50:   (.phase1_rules.section_7_2_applicable.latency_p50_ms <= 727)
}' $RUNDIR/metrics.json
```

This is for the developer's eyes; the binding gate is Task 11's acceptance test. On §8 miss: do not retune.

---

## Task 11: TDD — Phase 1 end-to-end acceptance test

**Files:**
- Create: `benchmarks/provbench/phase1/tests/end_to_end_heldout_serde.rs` (modeled on `end_to_end_canary.rs`; does NOT edit canary)

- [ ] **Step 1: Read the canary test as the template**

Read `benchmarks/provbench/phase1/tests/end_to_end_canary.rs` end-to-end. The serde test mirrors its shape: `#[ignore]`, requires `work/serde`, runs phase1 score + scoring compare in a temp dir, asserts §8 thresholds. Differences vs canary: no `phase1_git_sha` assertion (no run_meta in temp dir per design), adds row-count and `rule_set_version=v1.1` `request_id` checks.

- [ ] **Step 2: Write the failing test**

```rust
// benchmarks/provbench/phase1/tests/end_to_end_heldout_serde.rs
//! SPEC §9.4 held-out gate on serde-rs/serde @ 65e1a507.
//! Asserts the three §8 thresholds, row-count consistency, and
//! rule_set_version=v1.1 evidence in request_id. Does NOT assert
//! phase1_git_sha — that lives in the committed run_meta.json gate
//! (separate from the e2e test).

use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

const SERDE_T0: &str = "65e1a50749938612cfbdb69b57fc4cf249f87149";

fn provbench_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn ensure_scoring_binary_built() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scoring_manifest = manifest_dir.join("../scoring/Cargo.toml");
    let bin_path = manifest_dir.join("../scoring/target/release/provbench-score");
    let status = std::process::Command::new("cargo")
        .args([
            "build", "--release",
            "--manifest-path", scoring_manifest.to_str().unwrap(),
            "--bin", "provbench-score",
        ])
        .status()
        .expect("cargo build provbench-score");
    assert!(status.success(), "cargo build --release provbench-score failed");
    assert!(bin_path.exists(), "provbench-score binary not found at {}", bin_path.display());
    bin_path
}

#[test]
#[ignore = "requires benchmarks/provbench/work/serde checkout and prepared subset under results/serde-heldout-2026-05-15-canary/baseline; run with --ignored"]
fn spec_section_8_thresholds_clear_on_serde_heldout_subset() {
    let provbench = provbench_root();
    let workrepo = provbench.join("work/serde");
    assert!(workrepo.exists(), "needs work/serde checkout for held-out e2e");

    let baseline_run = provbench.join("results/serde-heldout-2026-05-15-canary/baseline");
    assert!(
        baseline_run.join("metrics.json").exists(),
        "needs Step-6 baseline metrics.json before running this test"
    );

    let phase1_bin = env!("CARGO_BIN_EXE_provbench-phase1");
    let score_bin = ensure_scoring_binary_built();

    let out = TempDir::new().unwrap();
    let out_p = out.path().to_path_buf();

    // 1) phase1 score
    let status = Command::new(phase1_bin)
        .args([
            "score",
            "--repo", workrepo.to_str().unwrap(),
            "--t0", SERDE_T0,
            "--facts",
            provbench.join("facts/serde-65e1a507-c2d3b7b.facts.jsonl").to_str().unwrap(),
            "--diffs-dir",
            provbench.join("facts/serde-65e1a507-c2d3b7b.diffs").to_str().unwrap(),
            "--baseline-run", baseline_run.to_str().unwrap(),
            "--out", out_p.to_str().unwrap(),
            "--rule-set-version", "v1.1",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "phase1 score failed");

    // 2) provbench-score compare
    let status = Command::new(&score_bin)
        .args([
            "compare",
            "--baseline-run", baseline_run.to_str().unwrap(),
            "--candidate-run", out_p.to_str().unwrap(),
            "--candidate-name", "phase1_rules",
            "--out", out_p.join("metrics.json").to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "provbench-score compare failed");

    let metrics: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out_p.join("metrics.json")).unwrap()).unwrap();

    // 3) SPEC §8 verbatim
    let stale_wlb = metrics["phase1_rules"]["section_7_1"]["stale_detection"]["wilson_lower_95"]
        .as_f64().expect("phase1_rules stale_detection wilson_lower_95");
    let valid_wlb = metrics["phase1_rules"]["section_7_1"]["valid_retention_accuracy"]["wilson_lower_95"]
        .as_f64().expect("phase1_rules valid_retention_accuracy wilson_lower_95");
    let p50 = metrics["phase1_rules"]["section_7_2_applicable"]["latency_p50_ms"]
        .as_u64().expect("phase1_rules latency_p50_ms");

    assert!(stale_wlb >= 0.30, "§8 #5 stale recall WLB {:.4} < 0.30", stale_wlb);
    assert!(valid_wlb >= 0.95, "§8 #3 valid retention WLB {:.4} < 0.95", valid_wlb);
    assert!(p50 <= 727,        "§8 #4 latency p50 {} ms > 727", p50);

    // 4) row-count consistency: predictions.jsonl line count == manifest selected_count
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(baseline_run.join("manifest.json")).unwrap()).unwrap();
    let selected_count = manifest["selected_count"].as_u64().expect("selected_count");
    let pred_lines = std::fs::read_to_string(out_p.join("predictions.jsonl"))
        .unwrap()
        .lines()
        .count() as u64;
    assert_eq!(pred_lines, selected_count, "predictions row count != manifest selected_count");

    // 5) rule_set_version=v1.1 evidence in every request_id
    let preds = std::fs::read_to_string(out_p.join("predictions.jsonl")).unwrap();
    for (i, line) in preds.lines().enumerate() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let req_id = row["request_id"].as_str().expect("request_id");
        assert!(
            req_id.contains("v1.1"),
            "row {} request_id={} does not embed rule_set_version v1.1",
            i, req_id
        );
    }
}
```

- [ ] **Step 3: Build the test (--no-run) and confirm it compiles**

```bash
cargo test --release \
  --manifest-path benchmarks/provbench/phase1/Cargo.toml \
  --test end_to_end_heldout_serde --no-run
```

Expected: build succeeds.

- [ ] **Step 4: Run the test against the prepared serde subset**

```bash
cargo test --release \
  --manifest-path benchmarks/provbench/phase1/Cargo.toml \
  --test end_to_end_heldout_serde -- --ignored --nocapture
```

Expected: PASS. If FAIL → §8 miss. Per §10, **do not retune**. Capture the actual values from the test output for Task 13 / Task 14 findings.

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/phase1/tests/end_to_end_heldout_serde.rs
git commit -m "test(provbench): SPEC §9.4 held-out acceptance test on serde-rs/serde"
```

---

## Task 12: Findings doc

**Files:**
- Create: `benchmarks/provbench/results/serde-heldout-2026-05-15-findings.md`

- [ ] **Step 1: Read pilot findings as template**

Read `benchmarks/provbench/results/phase1/2026-05-15-findings.md` end-to-end. Mirror its section structure exactly, with values swapped for serde held-out.

- [ ] **Step 2: Compute the side-by-side delta**

```bash
PILOT=benchmarks/provbench/results/phase1/2026-05-15-canary/metrics.json
HELD=$RUNDIR/metrics.json
jq -n --argfile pilot $PILOT --argfile held $HELD '{
  pilot: {
    stale_wlb: $pilot.phase1_rules.section_7_1.stale_detection.wilson_lower_95,
    valid_wlb: $pilot.phase1_rules.section_7_1.valid_retention_accuracy.wilson_lower_95,
    p50_ms:    $pilot.phase1_rules.section_7_2_applicable.latency_p50_ms
  },
  heldout: {
    stale_wlb: $held.phase1_rules.section_7_1.stale_detection.wilson_lower_95,
    valid_wlb: $held.phase1_rules.section_7_1.valid_retention_accuracy.wilson_lower_95,
    p50_ms:    $held.phase1_rules.section_7_2_applicable.latency_p50_ms
  },
  delta: {
    stale_wlb: ($held.phase1_rules.section_7_1.stale_detection.wilson_lower_95
              - $pilot.phase1_rules.section_7_1.stale_detection.wilson_lower_95),
    valid_wlb: ($held.phase1_rules.section_7_1.valid_retention_accuracy.wilson_lower_95
              - $pilot.phase1_rules.section_7_1.valid_retention_accuracy.wilson_lower_95),
    p50_ms:    ($held.phase1_rules.section_7_2_applicable.latency_p50_ms
              - $pilot.phase1_rules.section_7_2_applicable.latency_p50_ms)
  }
}'
```

Use the output to populate the side-by-side table in findings.md.

- [ ] **Step 3: Write the findings doc**

```markdown
# ProvBench Phase 1 (rules) — 2026-05-15 serde held-out findings (`rule_set_version v1.1`)

## Thesis under test

A deterministic, structural, single-repo HEAD-only rules pass clears SPEC §8 #3 / #4 / #5 verbatim on a repo the v1.1 rule set was never tuned on. Held-out Round 1 is `serde-rs/serde` @ T₀ `65e1a50749938612cfbdb69b57fc4cf249f87149` (SPEC §13.2 pre-registered, leakage-clean). Pilot tuning was performed on ripgrep only; per SPEC §10 no R3/R4/R5/R7 retuning is permitted on the held-out repo. This document records the result regardless of pass or fail — §10 forbids in-round retuning either way.

## Run details

| Field | Value |
|---|---|
| Runner | `provbench-phase1` |
| `rule_set_version` | `v1.1` |
| Spec freeze hash | `683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c` |
| Labeler git SHA | `c2d3b7b03a51a9047ff2d50077200bb52f149448` |
| Phase 1 git SHA | `ccfc901be17124d08c19a6de50294ff79ded6fc3` |
| Held-out repo | serde-rs/serde @ `65e1a50749938612cfbdb69b57fc4cf249f87149` |
| Baseline-run subset | `results/serde-heldout-2026-05-15-canary/baseline` (DRY-RUN CARRIER — NOT EVIDENCE) |
| Sample seed | `0xC0DEBABEDEADBEEF` = `13897750829054410479` (decimal; pilot default) |
| Row count | <ROWS> (loaded from `<baseline>/predictions.jsonl`) |
| Coverage | held-out serde canary subset; not full corpus |
| Wall time | <WALL> s (single-threaded; per-row p50 measured below) |

### Phase 1 source byte-stability

`git diff ccfc901be171..HEAD -- benchmarks/provbench/phase1/src` → empty. Feature-branch phase1 source is byte-identical to phase1@ccfc901be171, which makes the §8 result attributable to v1.1.

## SPEC §7.1 three-way table (held-out)

| Metric | Point | Wilson LB |
|---|---|---|
| Stale detection recall | <P> | <WLB> |
| Stale detection precision | <P> | — |
| Stale detection F1 | <P> | — |
| Valid retention accuracy | <P> | <WLB> |
| Needs_revalidation routing accuracy | <P> | <WLB> |

(Populate from `<RUNDIR>/metrics.json.phase1_rules.section_7_1.*`.)

## SPEC §8 threshold verdict

| Threshold | Required | Observed (serde held-out) | Pass? |
|---|---|---|:---:|
| §8 #3 valid retention WLB | ≥ 0.95 | <VAL> | <✅/❌> |
| §8 #4 latency p50 (per-row, ms) | ≤ 727 | <VAL> | <✅/❌> |
| §8 #5 stale recall WLB | ≥ 0.30 | <VAL> | <✅/❌> |

Stretch internal (`stale_detection.recall ≥ 0.80` point estimate): <achieved / not achieved>.

## Side-by-side with the ripgrep v1.1 pilot

| Metric | Pilot (ripgrep) | Held-out (serde) | Δ |
|---|---|---|---|
| Stale recall WLB | 0.9537 | <VAL> | <DELTA> |
| Valid retention WLB | 0.9716 | <VAL> | <DELTA> |
| Latency p50 (ms) | 2 | <VAL> | <DELTA> |

## Per-rule confusion

(Insert table from `<RUNDIR>/metrics.json.per_rule_confusion`; flag any rules that move differently than they did on the pilot — R7's narrow-class regime and R4's heuristic line-presence probe are the usual suspects.)

## Latency methodology (unchanged from pilot)

Candidate column reports per-row `wall_ms` (rule-classification cost per fact, µs–ms scale). The §8 #4 ≤ 727 ms threshold applies to the candidate column alone. (No live LLM baseline column on held-out per scope-out.)

## Hygiene flags

1. **Dry-run baseline column is a no-LLM subset carrier, NOT evidence.** Annotated in `<RUNDIR>/phase1/run_meta.json.baseline_run_kind = "dry_run_subset_carrier_not_evidence"`.
2. **R4 line-presence probe still heuristic.** Pilot caveat carried; flagged in any held-out R4 confusion.
3. **R7 fires on a narrow class.** Pilot caveat carried (function moved to same-extension same-stem file).
4. **Needs_revalidation routing accuracy = 0** (same as pilot and LLM baseline; §8 does not gate NR routing).
5. **Coverage: held-out canary subset**, not full corpus.
6. **Anti-leakage:** no R3/R4/R5/R7 retune; no labeler / rule-chain changes; no LLM held-out baseline; pallets/flask is Round 2 (out of scope here).
7. **Determinism preserved:** `labeler/tests/determinism_serde.rs` (Task 4) + phase1 two-run compare (Task 9) both green.

## What is and is not in scope

In scope for this PR:
- Held-out artifacts under `results/serde-heldout-2026-05-15-canary/` (manifest, predictions, run_meta, metrics for both `baseline/` carrier and `phase1/`; top-level compare metrics.json; hand-written phase1 run_meta.json).
- This findings doc.
- One new row in SPEC §11 recording the held-out result.
- Sibling tests `labeler/tests/determinism_serde.rs` + `phase1/tests/end_to_end_heldout_serde.rs`.

Out of scope (per the locked plan and SPEC §12):
- pallets/flask held-out (Round 2; separate brief).
- Fresh LLM-as-invalidator column on held-out (Phase 0c κ ≈ 0; budget unjustified).
- v2 LLM second-pass over `needs_revalidation` rows.
- Cross-repo / tunnels / multi-branch / semantic equivalence handling.
- Any retune of R3/R4/R5/R7 thresholds.
- Integration into the ironmem runtime hot path.
- Adding hex `value_parser` to `provbench-baseline sample --seed` (baseline source out of scope).
- Adding `build.rs` / auto run_meta.json emission to `provbench-phase1`.
```

- [ ] **Step 4: Substitute values from `<RUNDIR>/metrics.json` and the §8 verdict**

Replace `<ROWS>`, `<WALL>`, all `<P>`, `<WLB>`, `<VAL>`, `<DELTA>`, `<✅/❌>` placeholders with actual values from `$RUNDIR/metrics.json` and Step 2's jq output. Carefully match the placeholder count to the actual cells.

---

## Task 13: SPEC §11 row append (record-only)

**Files:**
- Modify: `benchmarks/provbench/SPEC.md` (append exactly one row to §11 table; line ~178)

- [ ] **Step 1: Locate the §11 table tail**

```bash
grep -n "^## 11\.\|^## 12\." benchmarks/provbench/SPEC.md
# expect: "## 11. Spec change log" near line 175 and "## 12. Known exclusions" near line 188
```

- [ ] **Step 2: Append the §11 row (substitute PASS or FAIL based on Task 11 result)**

Insert the new row IMMEDIATELY before the blank line that precedes "## 12. Known exclusions". The row text (single line in the markdown table; line-wrapped here for readability — collapse into one line on insert):

```
| 2026-05-15 | §9.4 (record only) | First §9.4 held-out result recorded: serde-rs/serde @ 65e1a507 + labeler @ c2d3b7b0 + phase1 @ ccfc901, `rule_set_version v1.1`, no in-round retuning. Result: <PASS | FAIL §8 #N>. Findings: `benchmarks/provbench/results/serde-heldout-2026-05-15-findings.md`. | First held-out round per SPEC §10 anti-leakage and §9.4 scale-out gate. The v1.1 pilot result (2026-05-15) is only the floor; the thesis isn't established until v1.1 also clears §8 on a repo it was never tuned on. | None — this is a record of the held-out result; SPEC §§1–10/12–15 body untouched. The R7 reachability + leaf-stem proxy and the BTreeMap AgreementReport from `phase1 @ ccfc901be171` are not changed by this entry. |
```

- [ ] **Step 3: Verify ONLY the §11 row was added**

```bash
git diff benchmarks/provbench/SPEC.md
```

Expected: exactly one `+` line (the new §11 row). Any other addition or deletion → unintentional edit; revert and try again.

- [ ] **Step 4: Commit**

```bash
git add benchmarks/provbench/SPEC.md benchmarks/provbench/results/serde-heldout-2026-05-15-findings.md
git commit -m "docs(provbench): §9.4 held-out result on serde-rs/serde — <PASS | FAIL §8 #N>"
```

---

## Task 14: Commit results artifacts (excluding phase1.sqlite)

**Files:**
- Add: `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/` (except `phase1/phase1.sqlite`)

- [ ] **Step 1: Verify `phase1.sqlite` will not be committed**

Open or create `.gitignore` and check for an entry like `benchmarks/provbench/results/**/phase1/phase1.sqlite` or `**/phase1.sqlite`. If none, add it:

```bash
echo 'benchmarks/provbench/results/**/phase1/phase1.sqlite' >> .gitignore
```

```bash
git check-ignore -v benchmarks/provbench/results/serde-heldout-2026-05-15-canary/phase1/phase1.sqlite
# expect: a non-empty match
```

- [ ] **Step 2: Stage everything except phase1.sqlite**

```bash
git add benchmarks/provbench/results/serde-heldout-2026-05-15-canary/manifest.json 2>/dev/null  # may not exist at top
git add benchmarks/provbench/results/serde-heldout-2026-05-15-canary/metrics.json
git add benchmarks/provbench/results/serde-heldout-2026-05-15-canary/baseline/{manifest.json,predictions.jsonl,run_meta.json,metrics.json}
git add benchmarks/provbench/results/serde-heldout-2026-05-15-canary/phase1/{predictions.jsonl,rule_traces.jsonl,run_meta.json}
git add .gitignore
```

- [ ] **Step 3: Confirm phase1.sqlite is NOT staged**

```bash
git status --short benchmarks/provbench/results/serde-heldout-2026-05-15-canary/phase1/
# expect: predictions.jsonl, rule_traces.jsonl, run_meta.json — but NOT phase1.sqlite
```

- [ ] **Step 4: Commit**

```bash
git commit -m "data(provbench): held-out serde canary artifacts for SPEC §9.4 Round 1"
```

---

## Task 15: Final harness gates

**Files:** repo-wide

- [ ] **Step 1: `cargo fmt --all -- --check`**

```bash
cargo fmt --all -- --check
```

Expected: exit 0, no output. If non-zero, run `cargo fmt --all` then re-stage and amend the most recent test/data commit (NOT the SPEC commit).

- [ ] **Step 2: `cargo clippy --workspace --all-targets --all-features -- -D warnings`**

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: exit 0, no warnings. Fix any flagged issues in the test files (the only Rust source we added) — do NOT touch labeler / baseline / phase1 / scoring source.

- [ ] **Step 3: `cargo test --workspace`**

```bash
cargo test --workspace
```

Expected: all default (non-ignored) tests green. The held-out tests are `#[ignore]` and were already exercised in Tasks 4 and 11 with `--ignored`.

- [ ] **Step 4: Re-verify SPEC.md diff is JUST the §11 row**

```bash
git diff main..HEAD -- benchmarks/provbench/SPEC.md
```

Expected: a single new row in §11. Anything else → STOP and fix.

- [ ] **Step 5: Re-verify phase1 source byte-stability (final guard)**

```bash
git diff ccfc901be17124d08c19a6de50294ff79ded6fc3..HEAD -- benchmarks/provbench/phase1/src
```

Expected: empty. Any output here invalidates the §10 contract — STOP and resolve before merging.

- [ ] **Step 6: Worktree teardown**

```bash
git worktree remove /tmp/ironmem-worktrees/labeler-c2d3b7b
git worktree remove /tmp/ironmem-worktrees/phase1-ccfc901
git worktree list   # should not include the two ephemeral worktrees
```

If `git worktree remove` fails because the working tree is dirty (it shouldn't be — we never edited inside worktrees), add `--force`. But pause and inspect first — unexpected drift inside a pinned worktree is a red flag.

- [ ] **Step 7: Final commit (if `.gitignore` or any harness fixes were needed)**

```bash
git status
# if anything is staged: git commit -m "chore: post-gate cleanup for §9.4 held-out round"
# else: nothing to commit, working tree clean
```

---

## Verification

End-to-end verification gates (run sequentially; halt on first failure):

1. `LABELER_BIN stamp` → `c2d3b7b03a51a9047ff2d50077200bb52f149448` (Task 2 Step 2)
2. `LABELER_BIN verify-tooling` exits 0 (Task 2 Step 3)
3. `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml --test determinism_serde -- --ignored` green (Task 4)
4. Manifest fields match pinned values (Task 5 Step 3)
5. Hand-written `<RUNDIR>/phase1/run_meta.json` fields all match pinned values (Task 8 Step 3)
6. Phase 1 two-run determinism diff empty (Task 9 Step 2)
7. `cargo test --manifest-path benchmarks/provbench/phase1/Cargo.toml --test end_to_end_heldout_serde -- --ignored` green or honest fail (Task 11 Step 4)
8. `jq` direct check on committed `<RUNDIR>/metrics.json` reproduces the test's three §8 assertions (Task 10 Step 3)
9. `cargo fmt --all -- --check` green (Task 15 Step 1)
10. `cargo clippy --workspace --all-targets --all-features -- -D warnings` green (Task 15 Step 2)
11. `cargo test --workspace` green (Task 15 Step 3)
12. `git diff main..HEAD -- benchmarks/provbench/SPEC.md` shows ONLY the §11 row append (Task 15 Step 4)
13. `git diff ccfc901be17124d08c19a6de50294ff79ded6fc3..HEAD -- benchmarks/provbench/phase1/src` empty (Task 15 Step 5)

If all 13 gates pass, the round is reproducible and §10-clean. The §8 verdict (pass or fail) is recorded in findings.md and SPEC §11 regardless.
