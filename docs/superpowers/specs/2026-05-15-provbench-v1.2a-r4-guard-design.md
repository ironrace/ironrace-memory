# ProvBench v1.2a — R4 kind-conditional guard relaxation (pilot-only) — design

- **Date:** 2026-05-15
- **Round:** v1.2a (pilot-only; no held-out evidence produced this round)
- **Prerequisite read:** `benchmarks/provbench/results/serde-heldout-2026-05-15-findings.md` (v1.1 held-out FAIL on §8 #3) and `benchmarks/provbench/SPEC.md` (especially §8, §10, §11, §13.2).
- **Strict §10 reading governs.** Serde is burned as a tuning target. The Q1 diagnostic (this design's empirical justification) was performed on serde held-out predictions; per the agreed reading, serde is informationally cited only and never reduces a §8 threshold check.

## 0. Diagnostic that produced this design

Inspection of the 162 R4 `valid__stale` rows on the v1.1 serde held-out canary (`results/serde-heldout-2026-05-15-canary/phase1/predictions.jsonl` joined with `rule_traces.jsonl` and `facts/serde-65e1a507-c2d3b7b.facts.jsonl`):

| Bucket | n | % | Diagnosis |
|---|---:|---:|---|
| Guard rejected a still-exact match (`Field`, `nonws_len < 8`) | 132 | 81.5% | `t0_span` IS byte-identical in `post_blob`, but the `MIN_PROBE_NONWS_LEN = 8` floor fires first |
| Same, for `TestAssertion`, `nonws_len < 20` | 1 | 0.6% | Same root cause, different kind |
| Lex-equivalent change (whitespace-only / comment-only / both) | 12 | 7.4% | Real byte diff; `re.sub(r"\s+", " ")` + Rust comment-strip on both sides restores match |
| Structural change (`PublicSymbol` on `serde/src/lib.rs` reorg) | 17 | 10.5% | Original line content no longer at `line_span`; the symbol IS re-defined elsewhere in the file |

**Conclusion:** the failure mode is the guard, not the comparator. 133/162 (82.1%) are recoverable by relaxing the length floor on `Field` (and optionally `TestAssertion`). Lex normalization and chain-reorder are reserved for later rounds.

## 1. Scope

### In scope (v1.2a)

- One-file source change in `benchmarks/provbench/phase1/src/rules/r4_span_hash_changed.rs` — kind-conditional guard relaxation (Variant C below).
- `rule_set_version` bump `v1.1` → `v1.2`.
- Pilot retune + measurement on `work/ripgrep` @ existing T₀ (`af6b6c543b224d348a8876f0c06245d9ea7929c5`).
- Acceptance test update in `benchmarks/provbench/phase1/tests/end_to_end_pilot_ripgrep.rs` asserting v1.2 §8 thresholds verbatim, no-regression-vs-v1.1, and a false-Valid safety bound.
- v1.2a findings doc at `benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-findings.md`.
- One new row in SPEC §11 recording the v1.1 → v1.2a transition with the framing in §6 below.
- Two-run byte-identical determinism check on pilot (mirrors v1.1).

### Out of scope (v1.2a)

- **Held-out evaluation.** Flask is Python; the labeler does not yet support Python (`labeler/src/ast/mod.rs:2` and `labeler/src/resolve/mod.rs:3` mark Python as future work). Held-out validation is deferred to **v1.2b**, which begins with Python labeler bring-up. Until v1.2b lands, the §9.4 thesis row remains at "v1.1 FAIL serde; v1.2a is code-only, awaiting held-out evidence."
- **Serde re-run.** Per strict §10, serde is burned as a tuning target.
- **Labeler / corpus / `emit-facts` / `emit-diffs` / baseline / scoring changes.** The v1.1 pilot artifacts under `corpus/ripgrep-af6b6c54-c2d3b7b.jsonl` + `facts/ripgrep-af6b6c54-c2d3b7b.{facts.jsonl,diffs}` are reused verbatim.
- **Phase1 rules other than R4.** R0/R1/R2/R3/R5/R6/R7/R8/R9 untouched.
- **Lex normalization** (whitespace-collapse + Rust comment-strip in R4). Reserved for v1.3 if v1.2b leaves residual §8 #3 miss on flask.
- **R3-override-R4 chain reorder.** Same reservation as lex normalization.
- **SPEC §1–10 / §12–§15 body.** Frozen; not edited this round.
- **Ironmem runtime integration.** Unchanged.
- **LLM rerank / Phase 0c baseline / NR routing.** Unchanged.

### Kill criterion

If pilot ripgrep fails ANY of the three acceptance gates in §4, v1.2a is killed: no §11 row, no PR merge, no findings doc commit. The R4 guard logic returns to design.

## 2. The R4 change

### Current behavior (v1.1, `phase1/src/rules/r4_span_hash_changed.rs:80–83`)

```rust
let guard_passed = match ctx.fact.kind.as_str() {
    "TestAssertion" => nonws_len >= MIN_PROBE_NONWS_LEN_ASSERTION,
    _ => probe_has_leaf && nonws_len >= MIN_PROBE_NONWS_LEN,
};
```

Where `MIN_PROBE_NONWS_LEN = 8` and `MIN_PROBE_NONWS_LEN_ASSERTION = 20`.

### v1.2 behavior

```rust
let guard_passed = match ctx.fact.kind.as_str() {
    "TestAssertion" => nonws_len >= MIN_PROBE_NONWS_LEN_ASSERTION,
    "Field"         => probe_has_leaf,   // length floor removed
    _               => probe_has_leaf && nonws_len >= MIN_PROBE_NONWS_LEN,
};
```

### Variants weighed and rejected

| Variant | Logic | Captures | Rejected because |
|---|---|---:|---|
| A. Drop length guard wholesale | Skip `MIN_PROBE_NONWS_LEN` for all non-TestAssertion kinds | 132 | Expands false-Valid surface across kinds where the current guard is empirically working (e.g., `FunctionSignature`, `PublicSymbol`). |
| B. Replace length with uniqueness | Require `post.count(t0_span) == 1` | ~120 (lossy) | Over-rejects when the same field appears in two structs in one file (legitimate within-file repetition). |
| **C. Kind-conditional relaxation** | **Drop length floor for `Field` only; keep `probe_has_leaf` as sanity floor** | **132** | **Adopted.** Smallest surface, highest leverage, analytically tractable false-Valid surface. |
| D. Widen probe with surrounding context | When probe too short, expand to `lines[start-1..=end+1]` | 132 | Surrounding-line shifts may newly miss valid rows; harder to reason about than C. |

### Why `probe_has_leaf` is kept as a sanity floor

`probe_has_leaf` checks that the leaf identifier appears somewhere in `t0_span`. For `Field` facts the leaf IS the field name and IS in the line by definition (`c: C,` contains `c`). The check is a defense against labeler edge cases where `t0_span` is whitespace-only or otherwise pathological — not a discrimination signal. Removing it would let degenerate spans null-match any post blob.

### Why TestAssertion is left alone

Only 1/162 serde misroute was `TestAssertion`. The guard is doing useful work elsewhere (test corpora are noisy; trivial `assert!(x);` lines would null-match liberally without the length floor). One row is not enough signal to widen the change.

### Why `PublicSymbol` and other kinds are unchanged

The 17 `PublicSymbol` structural-change rows on `serde/src/lib.rs` fail the **post-substring check itself**, not the guard. The original lines at `line_span` are now totally different content; the symbol exists elsewhere in the file (re-export reshuffle). Fixing them requires a different mechanism (R3-override-R4 chain reorder, or AST-symbol-still-exists fallback), reserved for v1.3.

## 3. Pilot run protocol

Pilot retune is permitted by SPEC §10. Held-out is not run this round (see §1 Out of scope).

### Inputs (all reused from v1.1; no regeneration)

- **Repo:** `work/ripgrep`, T₀ = `af6b6c543b224d348a8876f0c06245d9ea7929c5`, HEAD = whatever v1.1 used (first-parent descendant of T₀; identical to v1.1's `work/ripgrep` HEAD).
- **Corpus:** `benchmarks/provbench/corpus/ripgrep-af6b6c54-c2d3b7b.jsonl` (labeler @ `c2d3b7b03a51a9047ff2d50077200bb52f149448`).
- **Facts:** `benchmarks/provbench/facts/ripgrep-af6b6c54-c2d3b7b.facts.jsonl` (labeler @ `ababb376f7cf3f92c36dde6035d90932e083517a`).
- **Diffs:** `benchmarks/provbench/facts/ripgrep-af6b6c54-c2d3b7b.diffs` (labeler @ `ababb376f7cf3f92c36dde6035d90932e083517a`).

The dual-labeler-pin pattern (corpus @ `c2d3b7b0`, emit-facts/diffs @ `ababb376`) is intentional and matches the v1.1 pilot exactly — see `project_provbench_labeler_pin_quirks` memory and v1.1 findings hygiene flag 1.

### Steps

1. Build `provbench-phase1` from the v1.2a feature branch HEAD. Record phase1 git SHA in `run_meta.json`.
2. `provbench-baseline sample` with seed `0xC0DEBABEDEADBEEF` (the v1.1 default) and per-stratum targets `valid:2000, stale_changed:2000, stale_deleted:2000, stale_renamed:usize::MAX, needs_revalidation:2000`. Output to `results/ripgrep-pilot-2026-05-15-v1.2a-canary/baseline/`.
3. `provbench-phase1 run` against the inputs above. Output to `results/ripgrep-pilot-2026-05-15-v1.2a-canary/phase1/`.
4. Re-run step 3 with the same inputs. Assert byte-identical `predictions.jsonl` across runs (modulo per-row `wall_ms`).

### Operational budget

Pilot ripgrep worst-case at the default per-stratum targets was $29.50 in v1.1 — within the default $25 budget after dry-run gating. v1.2a uses the same `--dry-run` posture as v1.1 (no LLM calls, no actual cost). No budget bump required.

## 4. Acceptance gates

All three are asserted inside `phase1/tests/end_to_end_pilot_ripgrep.rs` against the v1.2a pilot output. Failure of any gate fails the test and kills the round.

### Gate 1 — §8 verbatim

- `stale_detection.wilson_lower_95 ≥ 0.30` (§8 #5; operationalization carried forward from v1.1 / serde findings).
- `valid_retention.wilson_lower_95 ≥ 0.95` (§8 #3).
- `latency_p50_ms ≤ 727` (§8 #4; operationalized as "10× faster than LLM baseline" per v1.1 / serde findings — SPEC §8 text gives the ratio, the absolute ms threshold is the LLM baseline number divided by 10).

Note: SPEC §8 #4 has no p95 threshold; p95 is reported informationally only.

### Gate 2 — No regression vs v1.1 pilot

Targets from the v1.1 pilot artifacts (`benchmarks/provbench/results/phase1/2026-05-15-canary/metrics.json` and `benchmarks/provbench/results/phase1/2026-05-15-findings.md`):

- `stale_detection.wilson_lower_95 ≥ 0.9537` (v1.1 value; v1.2a must not be lower).
- `valid_retention.wilson_lower_95 ≥ 0.9716`.
- `latency_p50_ms` within +5 ms of v1.1's 2 ms (i.e. ≤ 7 ms).

### Gate 3 — False-Valid safety bound from dropped guard

The dropped length guard on `Field` widens the surface for `GT=StaleSourceChanged → prediction=Valid` (real stale missed as Valid because a coincidentally-identical short Field line exists in post). Quantitative bound, computed at v1.2a measurement time from both pilot artifacts:

- `count_v1.2a(stalesourcechanged__valid where fact.kind == "Field")` ≤ `count_v1.1(stalesourcechanged__valid where fact.kind == "Field")` + 20.

The +20 slack is order-of-magnitude calibrated: v1.1 pilot had 95 `stalesourcechanged__valid` rows total across all kinds (from the serde findings side-by-side table); a +20 absolute increase concentrated in `Field` is the worst plausible cost of the dropped guard. If v1.2a exceeds this, the dropped guard is misbehaving and the design must revisit Variant B (uniqueness) or D (widened probe) before flask Round 2.

## 5. Determinism gate

- `phase1` run twice on identical input → byte-identical `predictions.jsonl` (modulo `wall_ms`).
- No new `#[ignore]` tests this round.
- Determinism is asserted in `end_to_end_pilot_ripgrep.rs`, not in a separate test.

## 6. Findings doc + SPEC §11 row

### Findings doc

Location: `benchmarks/provbench/results/ripgrep-pilot-2026-05-15-v1.2a-findings.md`.

Shape matches the v1.1 pilot findings doc (`benchmarks/provbench/results/phase1/2026-05-15-findings.md`), with:

- Run details table (runner, rule_set_version, spec freeze hash, labeler pins, phase1 git SHA, repo, T₀, sample seed, per-stratum targets, corpus size, selected subset, phase1 wall time, phase1 stats).
- §8 threshold verdict table.
- Side-by-side v1.1 → v1.2a delta table (the same shape as the serde findings doc's pilot-vs-held-out table, but pilot-vs-pilot).
- Per-rule confusion table (focus on R4: count of `valid__stale` and `stalesourcechanged__valid` for `Field` kind specifically, demonstrating Gate 3 satisfaction).
- Hygiene flags carried forward verbatim from v1.1 (labeler dual-pin, R4-line-presence-still-heuristic note updated to v1.2a state, R7 narrow-class note, NR routing = 0, coverage statement, anti-leakage statement, determinism statement).
- An explicit "v1.2b reservations" section listing the items in §1 Out of scope that are deferred — Python labeler, flask Round 2, lex-normalization-if-needed, chain-reorder-if-needed.

### SPEC §11 row

One new row, framed exactly as:

> v1.2a is a pilot-only round. R4 kind-conditional guard relaxation (drop `MIN_PROBE_NONWS_LEN` floor for `Field`; `probe_has_leaf` retained as sanity floor; `TestAssertion` and other kinds unchanged). Pilot ripgrep `rule_set_version v1.2` clears §8 verbatim and gates 2 and 3 of the design's acceptance protocol. No held-out evidence is produced this round; the §9.4 thesis row remains at "v1.1 FAIL serde, awaiting v1.2b held-out evidence." Re-running the §10 leakage clock against a held-out repo is deferred to v1.2b, which requires Python-labeler bring-up before flask Round 2 (`pallets/flask` @ `2f0c62f5e6e290843f03c1fa70817c7a3c7fd661`, pre-registered §13.2) can run. Serde is not re-evaluated under v1.2a per the strict §10 reading agreed in the v1.2a design.

## 7. v1.2b reservations (NOT acted on this round)

Recorded in the v1.2a findings doc so the v1.2b brainstorm has a reference. None of these are touched in v1.2a:

1. **Python labeler bring-up.** Tree-sitter Python grammar dependency; fact-kind mapping for Python (`Field` → dataclass attributes / class attributes; `FunctionSignature` → `def` / `async def`; `PublicSymbol` → top-level names + `__all__`; `TestAssertion` → `assert` statements / `pytest.raises`); replay determinism gate for Python sort-stability; Python fixture corpus.
2. **Flask T₀ verification.** Confirm `2f0c62f5e6e290843f03c1fa70817c7a3c7fd661` resolves and walks first-parent forward correctly (mirror of the serde Task 1 amendment for `work/serde` HEAD).
3. **Flask Round 2 §8 verbatim + R4 generalization check.** Identical acceptance shape to the v1.1 serde plan.
4. **Lex normalization** (whitespace-collapse + Rust/Python comment-strip in R4). Only added if v1.2b leaves residual §8 #3 miss on flask.
5. **R3-override-R4 chain reorder.** Only added if v1.2b leaves residual `PublicSymbol`-move misroutes on flask.

## 8. Anti-leakage statement

- **Strict §10 reading.** Serde is burned. The diagnostic on serde informed the *direction* of the v1.2 change (kind-conditional guard relaxation on `Field`), but no per-row threshold was tuned to serde data. The fix is a structural guard correction, not a knob.
- **No labeler changes.** Corpus and facts/diffs pins are byte-identical to v1.1.
- **No corpus regen.** Sample seed and per-stratum targets are byte-identical to v1.1.
- **No SPEC body changes.** Only §11 gets a new row.
- **No held-out evaluation this round.** v1.2a explicitly does not claim held-out evidence; the §9.4 row records pending status until v1.2b.
- **Phase1 source change is one file, one match arm.** The blame line from §8 #3 success/failure on the v1.2a pilot maps to exactly that one source edit.

## 9. Implementation footprint

- 1 source file modified: `phase1/src/rules/r4_span_hash_changed.rs` — one match arm added.
- 1 test file modified: `phase1/tests/end_to_end_pilot_ripgrep.rs` — three gate assertions updated.
- 1 new findings doc: `results/ripgrep-pilot-2026-05-15-v1.2a-findings.md`.
- 1 new SPEC §11 row.
- New canary results directory: `results/ripgrep-pilot-2026-05-15-v1.2a-canary/` (predictions, traces, rule traces, run_meta, phase1.sqlite, baseline carrier).

Total: ~6 paths touched. Single-rule round; single-rule blame line.
