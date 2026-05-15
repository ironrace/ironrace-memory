# ProvBench v1.2a R4 Kind-Conditional Guard — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drop the `MIN_PROBE_NONWS_LEN` length floor from R4's guard for `kind = "Field"` (keep `probe_has_leaf` as a sanity floor; leave `TestAssertion` and all other kinds unchanged); bump `rule_set_version` to `v1.2`; re-run pilot ripgrep; verify three acceptance gates; record results.

**Architecture:** Single-rule, single-file source change in `phase1/src/rules/r4_span_hash_changed.rs`. Test-driven: add two failing R4 unit tests (one positive: short Field probe now classified Valid when t0_span ∈ post; one safety: short Field probe with t0_span ∉ post still classified Stale), then implement the match-arm change to make them pass. Extend the existing end-to-end pilot test (`end_to_end_canary.rs`) with two additional gates (no-regression vs v1.1, false-Valid safety bound). Run pilot ripgrep with `--rule-set-version v1.2`, write findings doc, append SPEC §11 row.

**Tech Stack:** Rust (Cargo workspace under `benchmarks/provbench/`), `rusqlite`, `gix`, `tree-sitter` (R4 itself doesn't use tree-sitter — pure byte-substring on line slices), `tempfile`. Test harness: stock `cargo test` with `#[ignore]` for tests that need a `work/ripgrep` checkout.

**Design source:** `docs/superpowers/specs/2026-05-15-provbench-v1.2a-r4-guard-design.md` (committed at `ee77726`).

---

## File Inventory

**Modify:**
- `benchmarks/provbench/phase1/src/rules/r4_span_hash_changed.rs` — add `"Field" => probe_has_leaf,` arm to the match in `classify()`; update the module doc-comment.
- `benchmarks/provbench/phase1/tests/rules_unit.rs` — add two new R4 unit tests for the Field short-probe behavior.
- `benchmarks/provbench/phase1/tests/end_to_end_canary.rs` — add Gate 2 (no-regression vs v1.1) and Gate 3 (false-Valid safety bound) assertions.
- `benchmarks/provbench/SPEC.md` — append one row to §11 (no other section edits).

**Create:**
- `benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-canary/` — directory + run artifacts (created by the test run, then committed).
- `benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-findings.md` — findings doc.

**Do NOT modify (anti-leakage discipline):**
- `benchmarks/provbench/labeler/**` (corpus + facts/diffs pins are byte-stable from v1.1).
- `benchmarks/provbench/corpus/ripgrep-af6b6c54-c2d3b7b.jsonl`, `benchmarks/provbench/facts/ripgrep-af6b6c54-c2d3b7b.{facts.jsonl,diffs}`.
- `benchmarks/provbench/baseline/**` (sample seed, per-stratum targets unchanged).
- `benchmarks/provbench/scoring/**` (compare schema unchanged).
- `benchmarks/provbench/results/serde-heldout-2026-05-15-canary/**` (serde is burned; informationally cited only).
- `benchmarks/provbench/results/serde-heldout-2026-05-15-findings.md`.
- All other rules under `benchmarks/provbench/phase1/src/rules/r{0,1,2,3,5,6,7,8,9}_*.rs`.

---

## Task 1: Add failing R4 unit test — Field with short probe + t0_span in post → Valid

**Files:**
- Modify: `benchmarks/provbench/phase1/tests/rules_unit.rs` (append a new `#[test]` function after the existing `r4_stale_when_post_span_lines_differ_from_t0` test at line 194)

This test fails on v1.1 because the `Field` line `    c: C,\n` has `nonws_len = 4 < 8`, so R4's length guard rejects it and routes to Stale even though `t0_span ∈ post_blob`. After the v1.2 change it must classify as Valid.

- [ ] **Step 1: Write the failing test**

Append the following inside `benchmarks/provbench/phase1/tests/rules_unit.rs`, right after the `r4_stale_when_post_span_lines_differ_from_t0` test (after line 194):

```rust
/// v1.2 Field-guard fix: a single-character struct field at T0 like
/// `    c: C,\n` has nonws_len = 4, which the v1.1 R4 length floor
/// (MIN_PROBE_NONWS_LEN = 8) rejects — routing the row to Stale even
/// though the byte sequence is still present in post. v1.2 drops the
/// length floor for kind = "Field" (keeping `probe_has_leaf` as a
/// sanity floor); R4 must now classify this as Valid.
#[test]
fn r4_valid_when_short_field_probe_appears_unchanged_in_post() {
    let chain = RuleChain::default();
    let mut f = fact("Field", "irrelevant_hash");
    f.symbol_path = "S::c".into();
    f.line_span = [3, 3];
    let t0 = b"struct S {\n    a: A,\n    c: C,\n    d: D,\n}\n";
    // post inserts a new field above `c` so the line shifts but the
    // byte sequence `    c: C,\n` is still present verbatim.
    let post = b"struct S {\n    a: A,\n    b: B,\n    c: C,\n    d: D,\n}\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(t0)));
    assert_eq!(d, Decision::Valid);
    assert_eq!(rid, "R4");
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
cargo test -p provbench-phase1 --test rules_unit r4_valid_when_short_field_probe -- --nocapture
```

Expected: FAIL with an assertion mismatch — `Decision::Stale` returned where `Decision::Valid` expected, OR `rid` is something other than `"R4"` (e.g., R9 fallback). Either way, the test must fail before the source change.

- [ ] **Step 3: Do not commit yet** — the next task adds a second unit test, then both go together with the source fix.

---

## Task 2: Add failing safety unit test — Field with short probe + t0_span NOT in post → still Stale

**Files:**
- Modify: `benchmarks/provbench/phase1/tests/rules_unit.rs` (append a second new test right after the one added in Task 1)

This guards against the obvious failure mode of the v1.2 change: that dropping the length floor on Field would let a short probe null-match too liberally. The test gives a short Field probe whose bytes are NOT present in post — v1.2 must still classify it as Stale, not silently fall through.

- [ ] **Step 1: Write the failing test**

Append, after the Task 1 test:

```rust
/// v1.2 safety check for the dropped Field length floor: a short Field
/// probe whose byte sequence is NOT present in post must still route
/// to Stale, not silently fall through. Without this guarantee, the
/// `Field` arm would null-match degenerate post blobs.
#[test]
fn r4_stale_when_short_field_probe_absent_from_post() {
    let chain = RuleChain::default();
    let mut f = fact("Field", "irrelevant_hash");
    f.symbol_path = "S::c".into();
    f.line_span = [3, 3];
    let t0 = b"struct S {\n    a: A,\n    c: C,\n    d: D,\n}\n";
    // post replaces field `c: C,` with `c: NewType,` — the original
    // byte sequence `    c: C,\n` is gone.
    let post = b"struct S {\n    a: A,\n    c: NewType,\n    d: D,\n}\n";
    let (d, rid, _, _) = chain.classify_first_match(&ctx(&f, Some(post), Some(t0)));
    assert_eq!(d, Decision::Stale);
    assert_eq!(rid, "R4");
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
cargo test -p provbench-phase1 --test rules_unit r4_stale_when_short_field_probe_absent -- --nocapture
```

Expected behavior on v1.1: this test will **fail** because R4's guard rejects the short probe entirely (returns the `stale_source_changed` reason via the "guard failed" fall-through). Wait — re-read the v1.1 code: when the guard fails, R4 unconditionally returns `Decision::Stale` with `reason: "stale_source_changed"`. So this specific case already routes to Stale on v1.1 by accident. The test name + assertion `rid == "R4"` is still useful: it locks in the v1.2 behavior that R4 must be the rule that fires (not R9 fallback), so the safety guarantee is on the right rule. Run the test:

Expected: PASS on v1.1 (current behavior already returns Stale + R4 for this case). PASS on v1.2 after the change. The test is a **regression guard** rather than a failing-then-passing TDD test. Mark it as such in the comment.

If the test fails on v1.1, that means the chain is returning something other than R4 — investigate before proceeding.

- [ ] **Step 3: Do not commit yet** — the source change in Task 3 makes Task 1 pass; this task's test acts as a regression guard.

---

## Task 3: Apply the v1.2 R4 guard change

**Files:**
- Modify: `benchmarks/provbench/phase1/src/rules/r4_span_hash_changed.rs` (lines 80–83 of the existing `match` block; also update the module doc-comment at lines 1–26 to mention the v1.2 Field carve-out)

- [ ] **Step 1: Apply the match-arm edit**

Replace the existing block at `benchmarks/provbench/phase1/src/rules/r4_span_hash_changed.rs:80–83`:

```rust
        let guard_passed = match ctx.fact.kind.as_str() {
            "TestAssertion" => nonws_len >= MIN_PROBE_NONWS_LEN_ASSERTION,
            _ => probe_has_leaf && nonws_len >= MIN_PROBE_NONWS_LEN,
        };
```

with:

```rust
        let guard_passed = match ctx.fact.kind.as_str() {
            "TestAssertion" => nonws_len >= MIN_PROBE_NONWS_LEN_ASSERTION,
            // v1.2: drop length floor for `Field` — the leaf-symbol
            // presence check is the sanity floor. The v1.1 floor of 8
            // rejected exact-byte matches on short single-letter
            // fields (`c: C,`, `t: T,`) that drove 132 of 162 R4
            // false-Stale rows on the serde §9.4 held-out canary.
            // See docs/superpowers/specs/2026-05-15-provbench-v1.2a-r4-guard-design.md.
            "Field" => probe_has_leaf,
            _ => probe_has_leaf && nonws_len >= MIN_PROBE_NONWS_LEN,
        };
```

- [ ] **Step 2: Update the module doc-comment**

Find the existing module doc-comment block at the top of `r4_span_hash_changed.rs` (the `//!` lines 1–26). Right after the existing block, before the `const MIN_PROBE_NONWS_LEN: usize = 8;` line, add:

```rust
//! ## v1.2 Field carve-out
//!
//! The `MIN_PROBE_NONWS_LEN = 8` floor is skipped for `kind = "Field"`;
//! `probe_has_leaf` alone is the sanity gate. Empirical justification:
//! 132 of 162 R4 false-Stale rows on the serde §9.4 held-out canary
//! were `Field` facts like `'    c: C,\n'` (nonws_len = 4) whose
//! `t0_span` was byte-identical in `post_blob` — the comparator
//! was correct, the length floor rejected it. Other kinds are
//! unchanged: `TestAssertion` keeps its `MIN_PROBE_NONWS_LEN_ASSERTION`
//! floor, and all other kinds keep the v1.1 `probe_has_leaf &&
//! nonws_len >= 8` gate.
```

- [ ] **Step 3: Run the R4 unit tests to verify they all pass**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
cargo test -p provbench-phase1 --test rules_unit r4_ -- --nocapture
```

Expected: ALL R4 tests pass, including the two new ones from Tasks 1 and 2, AND the existing `r4_valid_when_t0_span_appears_unchanged_in_post` + `r4_stale_when_post_span_lines_differ_from_t0` + `r4_fires_when_span_hash_changes_no_whitespace_only_escape` tests (no regressions on the v1.1 R4 contract for non-Field kinds).

- [ ] **Step 4: Run the full phase1 unit suite (no `--ignored`)**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
cargo test -p provbench-phase1 -- --nocapture
```

Expected: all non-ignored tests pass.

- [ ] **Step 5: Run formatter + clippy gates**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: both green.

- [ ] **Step 6: Commit**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
git add benchmarks/provbench/phase1/src/rules/r4_span_hash_changed.rs benchmarks/provbench/phase1/tests/rules_unit.rs
git commit -m "$(cat <<'EOF'
feat(provbench): v1.2 R4 Field-kind guard relaxation

Drop MIN_PROBE_NONWS_LEN length floor for kind=Field; keep probe_has_leaf
as sanity floor. TestAssertion and other kinds unchanged. Closes 132 of
162 R4 false-Stale rows on the serde §9.4 held-out canary where short
Field probes had byte-identical t0_span in post but were rejected at
the v1.1 length gate.

Two new R4 unit tests: positive (Field short probe + t0_span in post →
Valid) and safety (Field short probe + t0_span NOT in post → still
Stale).

See docs/superpowers/specs/2026-05-15-provbench-v1.2a-r4-guard-design.md.
EOF
)"
```

---

## Task 4: Locate and record the v1.1 pilot baseline numbers needed for Gate 2 / Gate 3

This is a read-only task. The v1.1 pilot acceptance gates depend on numeric thresholds from the v1.1 pilot artifacts. We need them as constants in `end_to_end_canary.rs`.

**Files:**
- Read: `benchmarks/provbench/results/phase1/2026-05-15-canary/metrics.json` (v1.1 pilot metrics — the source of truth for the v1.1 thresholds)
- Read: `benchmarks/provbench/results/phase1/2026-05-15-findings.md` (cross-check)

- [ ] **Step 1: Extract v1.1 pilot threshold values**

Run:

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
python3 <<'PY'
import json
m = json.load(open("benchmarks/provbench/results/phase1/2026-05-15-canary/metrics.json"))
col = m.get("phase1_rules") or m
sec71 = col.get("section_7_1", col)
print("stale_recall_wlb_v1_1   =", sec71["stale_detection"]["wilson_lower_95"])
print("valid_retention_wlb_v1_1=", sec71["valid_retention_accuracy"]["wilson_lower_95"])
sec72 = col.get("section_7_2_applicable", col)
print("latency_p50_ms_v1_1     =", sec72["latency_p50_ms"])
print("latency_p95_ms_v1_1     =", sec72["latency_p95_ms"])
PY
```

Expected output (per the v1.1 findings doc):

```
stale_recall_wlb_v1_1   = 0.9537
valid_retention_wlb_v1_1= 0.9716
latency_p50_ms_v1_1     = 2
latency_p95_ms_v1_1     = 21
```

If the metrics.json schema differs from what the script expects, look at the file structure with `python3 -c 'import json; print(json.dumps(json.load(open("benchmarks/provbench/results/phase1/2026-05-15-canary/metrics.json")), indent=2)[:2000])'` and adapt the path.

- [ ] **Step 2: Extract v1.1 pilot `stalesourcechanged__valid` count for Field**

This requires joining v1.1 pilot `predictions.jsonl` with the corresponding `facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl` to look up `kind`. Run:

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory/benchmarks/provbench
python3 <<'PY'
import json
facts = {}
with open("facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl") as f:
    for line in f:
        d = json.loads(line); facts[d["fact_id"]] = d
n = 0
n_field = 0
with open("results/phase1/2026-05-15-canary/predictions.jsonl") as f:
    for line in f:
        p = json.loads(line)
        if p["ground_truth"] == "StaleSourceChanged" and p["prediction"] == "valid":
            n += 1
            kind = facts.get(p["fact_id"], {}).get("kind", "?")
            if kind == "Field":
                n_field += 1
print(f"v1.1 pilot stalesourcechanged__valid (all kinds): {n}")
print(f"v1.1 pilot stalesourcechanged__valid (Field only): {n_field}")
PY
```

Record both numbers. The Field-only count is the v1.1 baseline for Gate 3.

- [ ] **Step 3: Do not commit** — these numbers are used as constants in Task 5.

---

## Task 5: Extend `end_to_end_canary.rs` with Gate 2 (no-regression) and Gate 3 (false-Valid bound)

**Files:**
- Modify: `benchmarks/provbench/phase1/tests/end_to_end_canary.rs`

The test currently asserts only Gate 1 (SPEC §8 verbatim). v1.2a adds Gate 2 (no regression vs v1.1) and Gate 3 (false-Valid safety bound). The test must continue to be `#[ignore]` because it needs the `work/ripgrep` checkout.

We also need to thread `--rule-set-version v1.2` into the `provbench-phase1 score` invocation, and produce a side artifact (a `predictions.jsonl` we can post-process for Gate 3).

- [ ] **Step 1: Update the test signature and constants**

Open `benchmarks/provbench/phase1/tests/end_to_end_canary.rs`. At the top of the file, after the existing `use` statements (around line 4), add the v1.1 pilot baseline constants (use the values from Task 4):

```rust
// v1.1 pilot baseline values from
// benchmarks/provbench/results/phase1/2026-05-15-canary/metrics.json.
// These act as no-regression floors for v1.2a (Gate 2 of the v1.2a
// design protocol; see
// docs/superpowers/specs/2026-05-15-provbench-v1.2a-r4-guard-design.md).
const V1_1_STALE_RECALL_WLB: f64 = 0.9537;
const V1_1_VALID_RETENTION_WLB: f64 = 0.9716;
const V1_1_LATENCY_P50_MS: u64 = 2;

// Gate 3 (false-Valid safety bound from the dropped Field length guard):
// v1.2a must not increase the count of `stalesourcechanged__valid` for
// kind=Field by more than +20 vs the v1.1 pilot. The actual v1.1 Field
// count is loaded at test runtime from the v1.1 predictions to keep this
// resilient to changes in the v1.1 artifact (single source of truth).
const V1_2A_FIELD_FALSE_VALID_SLACK: usize = 20;
```

If the actual measured v1.1 numbers from Task 4 differ from `0.9537 / 0.9716 / 2`, use those measured values instead.

- [ ] **Step 2: Thread `--rule-set-version v1.2` into the phase1 invocation**

In the `phase1 score` command construction (around line 28–53), add `--rule-set-version`, `"v1.2"` to the args slice. The args block should read:

```rust
        .args([
            "score",
            "--repo",
            workrepo.to_str().unwrap(),
            "--t0",
            "af6b6c543b224d348a8876f0c06245d9ea7929c5",
            "--facts",
            provbench
                .join("facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl")
                .to_str()
                .unwrap(),
            "--diffs-dir",
            provbench
                .join("facts/ripgrep-af6b6c54-c2d3b7b.diffs")
                .to_str()
                .unwrap(),
            "--baseline-run",
            provbench
                .join("results/phase0c/2026-05-13-canary")
                .to_str()
                .unwrap(),
            "--out",
            out_p.to_str().unwrap(),
            "--rule-set-version",
            "v1.2",
        ])
```

(Verify the exact `--rule-set-version` flag spelling against `phase1/src/main.rs:53`. If the flag is named differently, use that name.)

- [ ] **Step 3: Add Gate 2 assertions**

After the existing `assert!(p95 >= p50, ...)` line (around line 121), and before the `assert!(stale_precision > 0.0 && stale_f1 > 0.0, ...)` block, add:

```rust
    // Gate 2 (no regression vs v1.1 pilot).
    assert!(
        stale_recall_wlb >= V1_1_STALE_RECALL_WLB,
        "Gate 2 regression: stale recall WLB {:.4} < v1.1 pilot {:.4}",
        stale_recall_wlb,
        V1_1_STALE_RECALL_WLB
    );
    assert!(
        valid_acc_wlb >= V1_1_VALID_RETENTION_WLB,
        "Gate 2 regression: valid retention WLB {:.4} < v1.1 pilot {:.4}",
        valid_acc_wlb,
        V1_1_VALID_RETENTION_WLB
    );
    assert!(
        p50 <= V1_1_LATENCY_P50_MS + 5,
        "Gate 2 regression: latency p50 {} ms > v1.1 pilot {} ms + 5 ms slack",
        p50,
        V1_1_LATENCY_P50_MS
    );
```

- [ ] **Step 4: Add Gate 3 helper function**

Add this helper at the bottom of the file (after the existing `ensure_scoring_binary_built` function):

```rust
/// Count `stalesourcechanged__valid` rows whose corresponding fact has
/// `kind = "Field"`. Joins a phase1 predictions.jsonl artifact with the
/// facts file used for the run.
fn count_stalesourcechanged_valid_field(
    predictions_path: &std::path::Path,
    facts_path: &std::path::Path,
) -> std::io::Result<usize> {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader};

    let mut kind_by_fact: HashMap<String, String> = HashMap::new();
    let facts_f = std::fs::File::open(facts_path)?;
    for line in BufReader::new(facts_f).lines() {
        let line = line?;
        let v: serde_json::Value = serde_json::from_str(&line)
            .expect("facts.jsonl row must be JSON");
        let fid = v["fact_id"].as_str().expect("fact_id");
        let kind = v["kind"].as_str().expect("kind");
        kind_by_fact.insert(fid.to_string(), kind.to_string());
    }

    let mut count = 0usize;
    let preds_f = std::fs::File::open(predictions_path)?;
    for line in BufReader::new(preds_f).lines() {
        let line = line?;
        let v: serde_json::Value = serde_json::from_str(&line)
            .expect("predictions.jsonl row must be JSON");
        let gt = v["ground_truth"].as_str().unwrap_or("");
        let pred = v["prediction"].as_str().unwrap_or("");
        if gt == "StaleSourceChanged" && pred == "valid" {
            let fid = v["fact_id"].as_str().unwrap_or("");
            if kind_by_fact.get(fid).map(|s| s.as_str()) == Some("Field") {
                count += 1;
            }
        }
    }
    Ok(count)
}
```

- [ ] **Step 5: Add Gate 3 assertion**

Inside the test, after the Gate 2 block from Step 3 and before the `for key in [...]` deltas loop, add:

```rust
    // Gate 3 (false-Valid safety bound from the dropped Field length
    // guard): v1.2a count of stalesourcechanged__valid for kind=Field
    // must not exceed the v1.1 pilot count by more than the slack.
    let v1_1_predictions = provbench.join("results/phase1/2026-05-15-canary/predictions.jsonl");
    let v1_2_predictions = out_p.join("predictions.jsonl");
    let facts_path = provbench.join("facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl");
    assert!(
        v1_1_predictions.exists(),
        "v1.1 pilot predictions.jsonl not found at {} — Gate 3 cannot compute baseline",
        v1_1_predictions.display()
    );
    assert!(
        v1_2_predictions.exists(),
        "v1.2 candidate predictions.jsonl not found at {} — phase1 score did not emit it",
        v1_2_predictions.display()
    );
    let n_v1_1_field = count_stalesourcechanged_valid_field(&v1_1_predictions, &facts_path)
        .expect("count v1.1 Field false-Valid");
    let n_v1_2_field = count_stalesourcechanged_valid_field(&v1_2_predictions, &facts_path)
        .expect("count v1.2 Field false-Valid");
    assert!(
        n_v1_2_field <= n_v1_1_field + V1_2A_FIELD_FALSE_VALID_SLACK,
        "Gate 3 violation: v1.2a Field false-Valid count {} > v1.1 pilot {} + slack {}",
        n_v1_2_field,
        n_v1_1_field,
        V1_2A_FIELD_FALSE_VALID_SLACK
    );
```

- [ ] **Step 6: Confirm `predictions.jsonl` location**

The test currently passes `--out out_p` to `provbench-phase1 score`. Verify the runner writes `predictions.jsonl` directly into `out_p` (not a subdirectory). Run a quick read of the runner's output path logic to confirm:

```bash
grep -n "out_predictions\|predictions.jsonl" /Users/jeffreycrum/git-repos/ironrace-memory/benchmarks/provbench/phase1/src/runner.rs /Users/jeffreycrum/git-repos/ironrace-memory/benchmarks/provbench/phase1/src/main.rs
```

If predictions.jsonl is written to a subdirectory of `out_p` (e.g. `out_p/phase1/predictions.jsonl`), adjust the `v1_2_predictions` path in Step 5 accordingly.

- [ ] **Step 7: Run formatter + clippy**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: both green. If clippy complains about the new helper, fix the warnings (do not allow `-D warnings` to be downgraded).

- [ ] **Step 8: Do NOT run the end-to-end test yet** — Task 6 runs it.

- [ ] **Step 9: Commit**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
git add benchmarks/provbench/phase1/tests/end_to_end_canary.rs
git commit -m "$(cat <<'EOF'
test(provbench): add v1.2a Gates 2 + 3 to pilot end-to-end test

Gate 2: no regression vs v1.1 pilot (stale recall WLB, valid retention
WLB, latency p50 with +5ms slack).
Gate 3: false-Valid safety bound from the dropped Field length guard —
count of stalesourcechanged__valid for kind=Field must not exceed v1.1
pilot count by more than +20.

Threads --rule-set-version v1.2 into the phase1 score invocation. Test
remains #[ignore] (needs work/ripgrep checkout).
EOF
)"
```

---

## Task 6: Run the v1.2a pilot end-to-end test (Gate 1 + Gate 2 + Gate 3)

This task executes the full pilot acceptance protocol against `work/ripgrep`. **This is the pass/fail moment for v1.2a.**

**Files:**
- Read-only: `benchmarks/provbench/work/ripgrep/` (must exist; the test will panic if it doesn't).

- [ ] **Step 1: Verify work/ripgrep exists**

```bash
ls /Users/jeffreycrum/git-repos/ironrace-memory/benchmarks/provbench/work/ripgrep/.git >/dev/null && echo "OK: ripgrep checkout present" || echo "MISSING: clone ripgrep into work/"
```

If missing, the user must clone ripgrep into `benchmarks/provbench/work/ripgrep` checked out at the v1.1 pilot HEAD (a first-parent descendant of T₀ `af6b6c543b224d348a8876f0c06245d9ea7929c5`). Stop and ask.

- [ ] **Step 2: Run the ignored test**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
cargo test -p provbench-phase1 --test end_to_end_canary --release -- --ignored --nocapture 2>&1 | tee /tmp/v1.2a-pilot-run.log
```

Expected: test PASSES (Gate 1 + Gate 2 + Gate 3 all satisfied).

- [ ] **Step 3: If the test fails — STOP and report**

If ANY gate fails, do NOT proceed to Task 7. Report the failing gate to the user verbatim:
- Gate 1 failure → §8 #3/#4/#5 regression; the v1.2 change is broken.
- Gate 2 failure → v1.1 pilot regression; the change broke a non-Field path.
- Gate 3 failure → dropped Field guard is misbehaving on stale_source_changed; revisit Variant B/D from the design.

The kill criterion from §1 of the design applies: no §11 row, no findings doc, no PR merge.

- [ ] **Step 4: If the test passes — capture the test's temp output**

The end-to-end test currently writes phase1 output into a `tempfile::TempDir` that is deleted after the test. To preserve the v1.2a canary artifacts for the findings doc + commit, re-run the phase1 pipeline manually with a fixed output path. Run:

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
PROVBENCH=$(pwd)/benchmarks/provbench
OUT=$PROVBENCH/results/ripgrep-pilot-2026-05-15-v1.2a-canary

# Build phase1 release binary
cargo build -p provbench-phase1 --release

mkdir -p "$OUT"

"$PROVBENCH/phase1/target/release/provbench-phase1" score \
  --repo "$PROVBENCH/work/ripgrep" \
  --t0 af6b6c543b224d348a8876f0c06245d9ea7929c5 \
  --facts "$PROVBENCH/facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl" \
  --diffs-dir "$PROVBENCH/facts/ripgrep-af6b6c54-c2d3b7b.diffs" \
  --baseline-run "$PROVBENCH/results/phase0c/2026-05-13-canary" \
  --out "$OUT" \
  --rule-set-version v1.2

cargo build --release --manifest-path "$PROVBENCH/scoring/Cargo.toml" --bin provbench-score

"$PROVBENCH/scoring/target/release/provbench-score" compare \
  --baseline-run "$PROVBENCH/results/phase0c/2026-05-13-canary" \
  --candidate-run "$OUT" \
  --candidate-name phase1_rules \
  --out "$OUT/metrics.json"

ls -la "$OUT"
```

Expected: `$OUT` contains `predictions.jsonl`, `rule_traces.jsonl`, `run_meta.json`, `phase1.sqlite`, and `metrics.json`. (Verify exact filename set against the v1.1 pilot canary directory `benchmarks/provbench/results/phase1/2026-05-15-canary/`.)

- [ ] **Step 5: Run a second time, assert byte-identical predictions (determinism gate)**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
PROVBENCH=$(pwd)/benchmarks/provbench
OUT2=$(mktemp -d)

"$PROVBENCH/phase1/target/release/provbench-phase1" score \
  --repo "$PROVBENCH/work/ripgrep" \
  --t0 af6b6c543b224d348a8876f0c06245d9ea7929c5 \
  --facts "$PROVBENCH/facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl" \
  --diffs-dir "$PROVBENCH/facts/ripgrep-af6b6c54-c2d3b7b.diffs" \
  --baseline-run "$PROVBENCH/results/phase0c/2026-05-13-canary" \
  --out "$OUT2" \
  --rule-set-version v1.2

# Determinism check: identical predictions (modulo per-row wall_ms).
python3 <<PY
import json
def strip(p):
    return [{k:v for k,v in json.loads(l).items() if k != "wall_ms"} for l in open(p)]
a = strip("$PROVBENCH/results/ripgrep-pilot-2026-05-15-v1.2a-canary/predictions.jsonl")
b = strip("$OUT2/predictions.jsonl")
assert a == b, "determinism gate FAILED: predictions differ between runs"
print(f"Determinism OK: {len(a)} predictions byte-identical (modulo wall_ms)")
PY

rm -rf "$OUT2"
```

Expected: prints `Determinism OK: <N> predictions byte-identical (modulo wall_ms)`. If it fails, halt — the rule chain is non-deterministic somewhere.

- [ ] **Step 6: Do not commit yet** — Task 7 creates the run_meta.json signature and findings doc, then commits everything together.

---

## Task 7: Write findings doc + run_meta + commit canary artifacts

**Files:**
- Create: `benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-canary/run_meta.json` (if not auto-emitted by phase1; check first)
- Create: `benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-findings.md`

- [ ] **Step 1: Check whether phase1 auto-emits run_meta.json**

```bash
ls /Users/jeffreycrum/git-repos/ironrace-memory/benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-canary/
```

If `run_meta.json` is present, skip to Step 3. If absent (per the labeler-pin-quirks memory: phase1 has no run_meta.json auto-emission), hand-write it in Step 2.

- [ ] **Step 2: If absent, hand-write run_meta.json**

Create `benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-canary/run_meta.json` with this content (substitute actual values from your environment):

```json
{
  "runner": "provbench-phase1",
  "rule_set_version": "v1.2",
  "spec_freeze_hash": "683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c",
  "labeler_git_sha_corpus": "c2d3b7b03a51a9047ff2d50077200bb52f149448",
  "labeler_git_sha_facts_and_diffs": "ababb376f7cf3f92c36dde6035d90932e083517a",
  "phase1_git_sha": "<output of: cd benchmarks/provbench/phase1 && git rev-parse HEAD>",
  "repo": "BurntSushi/ripgrep",
  "t0": "af6b6c543b224d348a8876f0c06245d9ea7929c5",
  "head": "<output of: cd benchmarks/provbench/work/ripgrep && git rev-parse HEAD>",
  "sample_seed": "0xC0DEBABEDEADBEEF",
  "per_stratum_targets": {
    "valid": 2000,
    "stale_changed": 2000,
    "stale_deleted": 2000,
    "stale_renamed": "usize::MAX",
    "needs_revalidation": 2000
  },
  "operational_budget_usd": 25,
  "phase1_wall_time_sec": null,
  "note": "Pilot v1.2a; identical inputs to v1.1 pilot except --rule-set-version v1.2 and phase1 source at the v1.2a Task 3 commit."
}
```

Use real values for `phase1_git_sha` (run `git rev-parse HEAD` from the workspace root after Task 3's commit), `head` (run `git -C benchmarks/provbench/work/ripgrep rev-parse HEAD`), and `phase1_wall_time_sec` (the elapsed wall time from Task 6 Step 4 — read from phase1 stderr or use the system clock).

- [ ] **Step 3: Compute the v1.1 → v1.2a delta numbers**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
python3 <<'PY'
import json
v11 = json.load(open("benchmarks/provbench/results/phase1/2026-05-15-canary/metrics.json"))
v12 = json.load(open("benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-canary/metrics.json"))

def pick(m, *path):
    cur = m
    for p in path:
        cur = cur[p]
    return cur

col = "phase1_rules"
fields = [
    ("stale_detection.recall.wilson_lower_95", ["section_7_1","stale_detection","wilson_lower_95"]),
    ("stale_detection.recall.point",           ["section_7_1","stale_detection","recall"]),
    ("stale_detection.precision",              ["section_7_1","stale_detection","precision"]),
    ("stale_detection.f1",                     ["section_7_1","stale_detection","f1"]),
    ("valid_retention.wilson_lower_95",        ["section_7_1","valid_retention_accuracy","wilson_lower_95"]),
    ("valid_retention.point",                  ["section_7_1","valid_retention_accuracy","point"]),
    ("latency_p50_ms",                         ["section_7_2_applicable","latency_p50_ms"]),
    ("latency_p95_ms",                         ["section_7_2_applicable","latency_p95_ms"]),
]
print(f"{'metric':45s}{'v1.1':>12s}{'v1.2a':>12s}{'delta':>12s}")
for name, path in fields:
    a = pick(v11[col], *path)
    b = pick(v12[col], *path)
    try: d = f"{b - a:+.4f}" if isinstance(a,float) else f"{b-a:+d}"
    except: d = "?"
    af = f"{a:.4f}" if isinstance(a,float) else str(a)
    bf = f"{b:.4f}" if isinstance(b,float) else str(b)
    print(f"{name:45s}{af:>12s}{bf:>12s}{d:>12s}")
PY
```

Record the output for the findings doc.

- [ ] **Step 4: Compute the per-rule confusion delta**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory/benchmarks/provbench
python3 <<'PY'
import json
from collections import Counter

facts = {json.loads(l)["fact_id"]: json.loads(l) for l in open("facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl")}

def load(preds_path, traces_path):
    preds = [json.loads(l) for l in open(preds_path)]
    last_rule = {}
    for l in open(traces_path):
        t = json.loads(l); last_rule[t["row_index"]] = t["rule_id"]
    return preds, last_rule

v11_p, v11_r = load("results/phase1/2026-05-15-canary/predictions.jsonl",
                     "results/phase1/2026-05-15-canary/rule_traces.jsonl")
v12_p, v12_r = load("results/ripgrep-pilot-2026-05-15-v1.2a-canary/predictions.jsonl",
                     "results/ripgrep-pilot-2026-05-15-v1.2a-canary/rule_traces.jsonl")

def confusion(preds, last_rule):
    c = Counter()
    for i, p in enumerate(preds):
        rid = last_rule.get(i, "?")
        gt_pred = (p["ground_truth"], p["prediction"])
        c[(rid, gt_pred)] += 1
    return c

c11 = confusion(v11_p, v11_r)
c12 = confusion(v12_p, v12_r)

# Focus: R4 misroutes
print("R4 valid__stale: v1.1 =", sum(v for (rid, (gt,pr)), v in c11.items() if rid=="R4" and gt=="Valid" and pr=="stale"),
      "v1.2a =", sum(v for (rid, (gt,pr)), v in c12.items() if rid=="R4" and gt=="Valid" and pr=="stale"))
print("R4 stalesourcechanged__valid: v1.1 =", sum(v for (rid, (gt,pr)), v in c11.items() if rid=="R4" and gt=="StaleSourceChanged" and pr=="valid"),
      "v1.2a =", sum(v for (rid, (gt,pr)), v in c12.items() if rid=="R4" and gt=="StaleSourceChanged" and pr=="valid"))

# Field-only Gate 3 numbers
def count_field_false_valid(preds):
    return sum(1 for p in preds if p["ground_truth"]=="StaleSourceChanged" and p["prediction"]=="valid"
               and facts.get(p["fact_id"],{}).get("kind")=="Field")
print("Field stalesourcechanged__valid: v1.1 =", count_field_false_valid(v11_p),
      "v1.2a =", count_field_false_valid(v12_p))
PY
```

Record output.

- [ ] **Step 5: Write the findings doc**

Create `benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-findings.md`. Use the v1.1 findings doc (`benchmarks/provbench/results/phase1/2026-05-15-findings.md`) as a shape template. The doc must contain:

1. **Header** — round name, rule_set_version, date, prerequisite read.
2. **Thesis under test** — short paragraph stating that the kind-conditional R4 guard relaxation does not regress v1.1 pilot metrics.
3. **§8 threshold verdict table** — three rows (§8 #3, #4, #5), with required and observed v1.2a values.
4. **Run details table** — runner, rule_set_version, spec freeze hash, labeler pins (corpus c2d3b7b0 / facts ababb376), phase1 git SHA, repo, T0, sample seed, per-stratum targets, corpus row count, selected subset, phase1 wall time.
5. **v1.1 → v1.2a side-by-side delta table** — from Step 3 above.
6. **Per-rule confusion delta table** — focus on R4 valid__stale and R4 stalesourcechanged__valid, with explicit Field-only Gate 3 numbers from Step 4.
7. **§8 #3 driver attribution** — short paragraph noting that v1.2a's R4 false-Stale rate dropped because the Field-kind length-floor was removed.
8. **Hygiene flags** — carry forward verbatim from v1.1: dual labeler pin (§9 quirks), R4-line-presence-still-heuristic note now updated to v1.2 Field-carve-out state, R7 narrow-class note, NR routing = 0, coverage statement, anti-leakage statement, determinism statement.
9. **v1.2b reservations** — explicit list of deferred items (Python labeler bring-up, flask Round 2, lex normalization if needed, R3-override-R4 chain reorder if needed).
10. **What is and is not in scope this round** — mirror the v1.1 serde findings doc shape.

Use specific values from Steps 3 and 4, not placeholders. If you cannot resolve a value, halt and ask.

- [ ] **Step 6: Run formatter on Markdown (skip — repo has no markdownlint gate; just spell-check by eye)**

- [ ] **Step 7: Commit canary artifacts + findings doc**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
git add benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-canary/
git add benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-findings.md
git commit -m "$(cat <<'EOF'
data(provbench): v1.2a pilot ripgrep canary artifacts + findings

rule_set_version v1.2. All three v1.2a acceptance gates clear:
 - Gate 1 (§8 verbatim): valid retention WLB <fill>, latency p50 <fill> ms,
   stale recall WLB <fill>.
 - Gate 2 (no regression vs v1.1): all v1.1 floors held.
 - Gate 3 (Field false-Valid bound): count <fill> ≤ v1.1 <fill> + 20.

Determinism: two-run byte-identical predictions (modulo wall_ms).

Labeler pin unchanged from v1.1 (corpus c2d3b7b0 / facts ababb376).
EOF
)"
```

Fill in the four `<fill>` placeholders with the actual measured values before committing.

---

## Task 8: Append SPEC §11 row

**Files:**
- Modify: `benchmarks/provbench/SPEC.md` (one new row in the §11 table at the bottom of the existing table, after the 2026-05-15 serde row at line 183)

- [ ] **Step 1: Compute the v1.2a SPEC §11 row content**

The row must use the v1.2a measured values from Task 7 Step 3. Use this exact framing (substitute `<measured-value>` placeholders with real numbers):

```
| 2026-05-15 | §11 (record only) | v1.2a pilot-only round recorded. R4 kind-conditional guard relaxation: `MIN_PROBE_NONWS_LEN` length floor dropped for `kind = "Field"` (one match arm added in `phase1/src/rules/r4_span_hash_changed.rs`; `probe_has_leaf` retained as sanity floor; `TestAssertion` and all other kinds unchanged). `rule_set_version v1.2`, phase1 git SHA `<sha>`, labeler pins unchanged (corpus `c2d3b7b0`, facts/diffs `ababb37`). Pilot ripgrep result: §8 #3 valid retention WLB `<v1.2a value>` (v1.1 was 0.9716), §8 #4 latency p50 `<v1.2a value>` ms (v1.1 was 2 ms), §8 #5 stale recall WLB `<v1.2a value>` (v1.1 was 0.9537). All three v1.2a acceptance gates clear: Gate 1 §8 verbatim, Gate 2 no regression vs v1.1, Gate 3 Field false-Valid count `<v1.2a value>` ≤ v1.1 pilot `<v1.1 value>` + 20 slack. Determinism: two-run byte-identical predictions (modulo wall_ms). Findings: `benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-findings.md`. | v1.2a is a pilot-only round. The §10 admission for R4 threshold tuning is consumed on ripgrep. No held-out evidence is produced this round; the §9.4 thesis row remains at "v1.1 FAIL serde, awaiting v1.2b held-out evidence." Per strict §10 reading agreed in the v1.2a design (`docs/superpowers/specs/2026-05-15-provbench-v1.2a-r4-guard-design.md`), serde is burned as a tuning target and is not re-evaluated this round. The Q1 diagnostic that surfaced the Field-kind length-floor failure mode used serde held-out predictions; the fix is a structural guard correction (not a per-row threshold tune), so this is accepted under the strict reading. Re-running the §10 leakage clock against a held-out repo is deferred to v1.2b, which requires Python-labeler bring-up before flask Round 2 (`pallets/flask` @ `2f0c62f5e6e290843f03c1fa70817c7a3c7fd661`, pre-registered §13.2) can run. | None for SPEC §§1–10 / §12–§15 (frozen body untouched). The §10 anti-leakage contract holds verbatim for held-out evaluation: serde is not retested under v1.2a; flask Round 2 is deferred to v1.2b which begins with Python labeler bring-up. Acceptance test `phase1/tests/end_to_end_canary.rs` updated to assert v1.2a Gates 1+2+3 verbatim against the v1.2 rule set and `work/ripgrep` (test remains `#[ignore]`; needs `work/ripgrep` checkout). |
```

- [ ] **Step 2: Read SPEC §11 to find the insertion point**

```bash
grep -n "^| 2026-05-15" /Users/jeffreycrum/git-repos/ironrace-memory/benchmarks/provbench/SPEC.md
```

Expected: identifies the 2026-05-15 serde §9.4 row. The v1.2a row goes immediately after it.

- [ ] **Step 3: Append the row**

Use the Edit tool to insert the v1.2a row directly after the existing 2026-05-15 serde row in `benchmarks/provbench/SPEC.md`. Do NOT modify any other row.

- [ ] **Step 4: Verify SPEC §1-§10 / §12-§15 are byte-stable**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
git diff benchmarks/provbench/SPEC.md
```

Expected: ONLY the §11 table has a new row. No other lines changed. If anything else is touched, revert and redo.

- [ ] **Step 5: Commit**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
git add benchmarks/provbench/SPEC.md
git commit -m "$(cat <<'EOF'
docs(provbench): SPEC §11 — record v1.2a pilot result

v1.2 rule set with kind-conditional R4 Field-guard relaxation clears
all three v1.2a acceptance gates on ripgrep pilot. No held-out evidence
this round (strict §10 reading); v1.2b begins with Python labeler bring-
up before flask Round 2. Serde is burned as a tuning target.

§§1-10 / §12-§15 byte-stable.
EOF
)"
```

---

## Task 9: Final verification + PR-ready check

**Files:** (read-only verification)

- [ ] **Step 1: Run the full test suite one more time (no `--ignored`)**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
cargo test --workspace -- --nocapture 2>&1 | tail -40
```

Expected: all default tests green.

- [ ] **Step 2: Run the ignored pilot end-to-end test one more time**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
cargo test -p provbench-phase1 --test end_to_end_canary --release -- --ignored --nocapture 2>&1 | tail -20
```

Expected: green.

- [ ] **Step 3: Format + clippy gate**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: both green.

- [ ] **Step 4: `git log` sanity check**

```bash
cd /Users/jeffreycrum/git-repos/ironrace-memory
git log --oneline -10
```

Expected: at minimum, four new commits on top of the branch base — Task 3 (R4 source + unit tests), Task 5 (end-to-end test gates), Task 7 (canary + findings), Task 8 (SPEC §11). Plus the design doc commit `ee77726` from the brainstorming.

- [ ] **Step 5: Report**

Summarize for the user: branch state, four new commits, Gate 1/2/3 verbatim numbers, findings doc path, SPEC §11 line number. Hand off for review or PR.

---

## What this plan does NOT do (kept out of scope explicitly)

- **No Python labeler.** All deferred to v1.2b.
- **No flask Round 2 run.** Same.
- **No serde re-run.** Strict §10.
- **No labeler / corpus / facts / diffs / scoring / baseline changes.** Anti-leakage discipline.
- **No phase1 rule changes outside R4.** Single-rule blame line.
- **No SPEC §1–10 / §12–§15 edits.** Frozen body.
- **No `run_meta.json` auto-emission feature work.** If `run_meta.json` is hand-written in Task 7, that's fine — adding auto-emission is a separate change.
- **No `--seed` hex parsing in provbench-baseline.** Per the labeler-pin-quirks memory, decimal-only is fine for this round (default seed is used).
- **No lex normalization (whitespace-collapse + comment-strip in R4).** Reserved for v1.3 if v1.2b leaves residual §8 #3 miss.
- **No R3-override-R4 chain reorder.** Same condition.
