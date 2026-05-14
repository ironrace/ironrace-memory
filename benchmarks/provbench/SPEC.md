# ProvBench-CodeContext: Benchmark Spec (v0, Phase 0a)

> **Status:** FROZEN 2026-05-09. Phase 0b/0c MUST NOT begin until this document is marked `Status: FROZEN` with a date and the spec hash recorded.
>
> **Purpose:** This document is the contract for evaluating provenance-driven invalidation in code-agent memory. Every fact type, label, rule, prompt, metric, threshold, and gate is fixed here before any labeler or system code is written. If a system change later requires a spec change, the change must be **dated, justified, and called out** in §11 — never silently absorbed.

---

## 1. Thesis under test

> Provenance-driven invalidation can prevent stale context from entering code-agent memory, approaching LLM-as-invalidator accuracy at much lower latency and cost.

The benchmark exists to test that thesis against a real baseline. Auditability, explainability, and provenance richness are *supporting* properties, not the thesis.

## 2. Unit of evaluation

- **Canonical row:** `fact_at_commit` — one fact, one commit, one ground-truth label, one system prediction.
- **Mutation event:** a commit that mutates one or more sources bound by at least one fact. Mutation events group `fact_at_commit` rows for latency and cost denominators.
- **Denominators:** every metric below specifies its denominator in `fact_at_commit` or `mutation_event` units. No metric is permitted without an explicit denominator.

## 3. Fact types

### 3.1 Included (v1, structural only)
1. **Function signature fact.** A claim of the form `function F has parameters P with return type R`. Bound to: source file, line span of the signature, fully qualified symbol name, content hash of the signature span at observation time.
2. **Class / struct field fact.** A claim of the form `type T has field N of type X`. Bound to: source file, line span of the field declaration, qualified field path, content hash.
3. **Public symbol existence fact.** A claim of the form `exported name N resolves in module M`. Bound to: module path, symbol path, content hash of the export site.
4. **README / API-doc claim referencing a resolvable symbol.** A claim of the form `doc D mentions symbol S`. Bound to: doc file, line span of the mention, qualified symbol name, content hash of both the mention span and the symbol's defining span.
5. **Test-assertion fact tied to a specific test function.** A claim of the form `test T asserts P about symbol S`. Bound to: test file, test function name, line span of the assertion, content hash of the assertion span and the asserted-on symbol's defining span.

### 3.2 Excluded (v1, may revisit)
- Free-text docstring claims about behavior (e.g. "handles malformed input gracefully").
- Performance / complexity claims (e.g. "runs in O(n)").
- Security / safety claims that depend on semantics.
- Any claim that requires a semantic-equivalence judgment to invalidate.

> **Why excluded:** Each requires LLM judgment to label, which puts the labeler in a circular evaluation against the LLM baseline. Structural-only is what makes the v1 thesis falsifiable.

## 4. Mutation labels (closed enum)

For each `fact_at_commit`, the ground-truth label is exactly one of:

| Label | Meaning |
|---|---|
| `valid` | No relevant change to bound source. |
| `stale_source_changed` | Bound span content hash changed. |
| `stale_source_deleted` | File removed, or symbol no longer resolves. |
| `stale_symbol_renamed` | Symbol renamed or moved (resolved by rename detection). |
| `needs_revalidation` | Bound span changed, but the change cannot be classified as one of the above by structural rules. Reported separately; never collapsed into stale or valid. |

Reason codes used at the system layer (`source_changed`, `source_deleted`, `symbol_renamed`, `derived_from_invalid`, `superseded`, `manual`, `rule_violation`) are distinct from labels and not benchmark ground truth.

## 5. Labeling rules

The labeler maps `(fact, commit) → label` deterministically. Procedure:

1. **Span resolution.** Locate the bound span in the commit's source tree. If the file is missing → `stale_source_deleted`.
2. **Symbol resolution.** Resolve the bound symbol via the project's tooling: `rust-analyzer` for Rust, `tree-sitter` + import graph for Python. Tooling versions are pinned in §13.1 and the resolved binary content hashes are recorded in the freeze record (§15). A tooling change requires a §11 entry and re-runs the leakage clock.
   - If the symbol no longer resolves and rename detection (Myers diff over symbol-bearing lines, ≥0.6 similarity, `git log --follow`-style) finds a renamed counterpart → `stale_symbol_renamed`.
   - If the symbol no longer resolves and no rename is detected → `stale_source_deleted`.
3. **Content hash check.**
   - `hash(span_content) == hash_at_observation` → `valid`.
   - `hash(span_content) != hash_at_observation`:
     - If the diff is whitespace-only or comment-only → `valid`.
     - If structural rules can classify the change (signature/field/symbol/test-assertion change) → `stale_source_changed`.
     - Otherwise → `needs_revalidation`.
4. **Tie-break.** Apply rules in the order above. The first match wins. No probabilistic blending.

The labeler is implemented in `benchmarks/provbench/labeler/` and is itself version-locked: changes to the labeler require re-running the spot-check (§9) and recording the labeler hash.

## 6. LLM-as-invalidator baseline

### 6.1 Prompt (frozen text, exact)

```
You are evaluating whether claims about source code are still supported
after a code change.

For each FACT in the FACTS list, decide one of:
  - "valid": the change does not affect the fact.
  - "stale": the change makes the fact no longer supported.
  - "needs_revalidation": the change is relevant but you cannot tell
    from structural information alone whether the fact still holds.

You must base your decision only on the DIFF and the FACT body.
Do not speculate about runtime behavior.

DIFF:
<unified diff, full file context for affected hunks>

FACTS:
<JSON array of {id, kind, body, source_path, line_span, symbol_path,
content_hash_at_observation}>

Respond with a JSON array of {id, decision} only. No prose.
```

### 6.2 Configuration
- **Model:** `claude-sonnet-4-6` pinned to API snapshot date `2026-05-09` (primary). Optional cross-check model: none for v1 freeze; adding a second model requires a §11 entry and a new freeze hash. Provider-side scoring shifts have happened mid-experiment before; identifier alone is not a freeze.
- **Temperature:** 0.
- **Max tokens:** 4096.
- **Retry policy:** up to 2 retries on transient errors (HTTP 5xx, rate-limit). On parse failure, retry once with the literal addendum `Your previous response was not valid JSON. Respond with a JSON array of {id, decision} only.`
- **Batching:** at most 32 facts per prompt to keep latency comparable to the system's per-query budget.
- **Cost accounting:** input + output tokens per call, summed per mutation event. **Token counts are the canonical cost unit.** Any $ figures in §7.2 must be derived from the 2026-05-09 token-price snapshot recorded in §15 — never re-priced after the fact.
- **Token-price snapshot.** Anthropic native API price for `claude-sonnet-4-6` on 2026-05-09: **$3.00 per 1M input tokens** and **$15.00 per 1M output tokens**. Source reference: Anthropic Sonnet 4.6 launch/pricing pages captured on 2026-05-09.
- **Phase 0c budget cap.** Pre-registered cap: **$250 USD**. The 0c run aborts if the cap is reached; the corpus is **not** silently truncated to fit budget.

### 6.3 Frozen at
Prompt text and configuration are frozen at spec freeze. Any change requires a §11 entry.

## 7. Metrics

### 7.1 Three-way reporting (mandatory)

Reporting a single global P/R is **prohibited** — it lets a conservative system dump hard cases into `needs_revalidation` and look better than it is. Always report:

- **Stale detection P/R.** Computed only over rows whose ground-truth label is in `stale_*`. A prediction counts as a true positive iff the system marked the row stale (any reason code). Denominator: rows with `stale_*` ground truth.
- **Valid retention accuracy.** Computed only over rows whose ground-truth label is `valid`. Accuracy = fraction the system also marks valid. Complement = false-invalidation rate. Denominator: rows with `valid` ground truth.
- **needs_revalidation routing accuracy.** Computed only over rows whose ground-truth label is `needs_revalidation`. Accuracy = fraction the system also routes to `needs_revalidation`. Denominator: rows with `needs_revalidation` ground truth.

### 7.2 Other metrics
- **Stale-exposure rate per agent action.** Numerator: agent actions in which a stale fact entered the assembled context bundle. Denominator: total agent actions in the harness run.
- **Calibration.** Reliability diagram over `confidence` for system predictions. 10-bucket binning. Reported as ECE.
- **Latency.** p50 and p95, separately for `ctx_query` and `ctx_revalidate`. Wall-clock per `mutation_event` for end-to-end revalidation.
- **Cost per correct invalidation.** Numerator: total tokens (LLM baseline) or compute-seconds (system) spent in the eval. Denominator: number of true-positive `stale_*` predictions. Reported separately for the system and the LLM baseline. Any $-denominated reporting derives from the §6.2 token-price snapshot only.

### 7.3 Reporting requirements
Every numeric result in any later writeup must specify the metric, denominator unit, repo split (pilot vs. held-out), and the spec freeze hash under which it was produced.

## 8. Pre-registered numeric thresholds

These are the gate values. They may be revised before spec freeze; **after freeze, they are immutable for v1.**

| Threshold | Value |
|---|---|
| Stale-detection recall vs. LLM baseline | within **10–15 points** |
| Stale-detection precision vs. LLM baseline | not worse by more than **10 points** |
| Valid-retention accuracy floor | **≥95%** (a system that fails this is shedding good context) |
| Latency p50 vs. LLM baseline | **≥10× faster** |
| Stale-exposure rate vs. citation-only baseline | **≥30% relative reduction** |

## 9. Acceptance gates

### 9.1 Phase 0b acceptance (pilot corpus)
- ≥95% agreement on a hand-spot-check of **200** random labels (n=50 has too wide a CI to defend a 95% gate; the 95% lower bound on n=50 dips below 86% even at a 95% point estimate).
- Report point estimate **and** the 95% lower CI bound. Gate is met iff the point estimate is ≥95%.
- Labeler determinism: a re-run on the same corpus + commits produces byte-identical labels.
- Spot-checker, labels, and disagreements recorded in `benchmarks/provbench/spotcheck/`.

### 9.2 Phase 0c acceptance (LLM baseline)
- Baseline numbers exist for all metrics in §7 over the pilot corpus.
- Numbers committed to the repo and tagged with the spec freeze hash and the LLM model version.

### 9.3 Week-4 kill / continue gate (Phase 3)
All four must hold to continue:
1. Recall threshold (§8) met.
2. Precision floor (§8) not violated.
3. Latency threshold (§8) met.
4. Stale-exposure improvement (§8) met.

If any fails: pivot to the engineering contribution ("fact freshness + source-hash invalidation for ironmem"). Decision recorded in writing.

### 9.4 Scale-out gate (Phase 4)
- Pilot kill gate cleared.
- Held-out repos prepared with the same labeler version.
- **No rule-threshold tuning** between pilot and held-out evaluation. Any retuning re-runs the leakage clock and disqualifies the held-out result for that round.

## 10. Anti-leakage

- **Repo-level split.** Pilot repo(s) for tuning; held-out repos for final reporting. Never mix.
- **Pre-committed repo list.** The exact pilot and held-out repos (with pinned commit SHAs at T₀) are written into §13.2 **before** the labeler first runs. Repo selection after seeing data is leakage and disqualifies the round.
- **Threshold freeze.** Rule thresholds (e.g. similarity floor for rename detection) are frozen at the end of pilot tuning. No held-out tuning.
- **Labeler version.** Same labeler version across pilot and held-out evaluations. Labeler changes invalidate prior held-out results.
- **Spec change clock.** Any change to this spec after Phase 0 freeze restarts the held-out evaluation.

## 11. Spec change log

| Date | Section | Change | Justification | Re-evals required |
|---|---|---|---|---|
| _(pre-freeze)_ | _all_ | initial draft | — | — |
| 2026-05-13 | §7.1 / §9.3 | First Phase 0c numeric result recorded (subset, 44%). Baseline runner gained skip-and-log on parse failure (commit `0b4d441`); batches 22–270 of the 2026-05-13 run produced under that binary, batches 1–21 under the pre-patch binary. Patch touches error-handling only; prompt + scoring byte-stable. | Five Sonnet responses returned non-JSON despite the addendum retry; aborting on each would have made any subset score impossible. The patch isolates the failure to a diagnostic sidecar so the run can complete to the budget cap. | None — the patch does not alter the prompt or scoring, and `tests/prompt_frozen.rs` covers prompt stability. Held-out repos (§9.4) and any future full-coverage run will use the post-patch binary by default. |

## 12. Known exclusions

Acknowledged out of scope for v1; **none** of these will be presented as evidence for or against the thesis without a follow-up spec.

- Multi-branch validity, merge / cherry-pick / rebase semantics.
- Multi-agent / multi-laptop sync; central audit plane.
- Cross-repo facts; tunnels.
- Semantic equivalence claims.
- Non-code facts (organizational knowledge, conversational memory).
- Performance and security claims.
- Latency under concurrent writers.

## 13. Files and locations

```
benchmarks/provbench/
  SPEC.md                  # this document
  labeler/                 # mechanical labeler (Phase 0b)
  baseline/                # LLM-as-invalidator runner (Phase 0c)
  corpus/                  # pilot corpus (Phase 0b output)
  spotcheck/               # hand-checks on pilot labels
  results/                 # numeric outputs, tagged by spec hash
```

### 13.1 Tooling pins

| Tool | Version / source | Content hash |
|---|---|---|
| `rust-analyzer` | `rust-analyzer 1.85.0 (4d91de4e 2025-02-17)`, rustup `stable-aarch64-apple-darwin` component | `f85740bfa5b9136e9053768c015c31a6c7556f7cfe44f7f9323965034e1f9aee` |
| `tree-sitter` CLI | `tree-sitter 0.25.6`, Homebrew binary at `/opt/homebrew/bin/tree-sitter` | `3e82f0982232f68fd5b0192caf4bb06064cc034f837552272eec8d67014edc5c` |
| `tree-sitter-python` grammar | npm package `tree-sitter-python@0.25.0`, tarball `https://registry.npmjs.org/tree-sitter-python/-/tree-sitter-python-0.25.0.tgz` | `63b76b3fa8181fd79eaad4abcdb21e2babcb504dbfc7710a89934fa456d26096` |
| `tree-sitter-rust` grammar | npm package `tree-sitter-rust@0.24.0`, tarball `https://registry.npmjs.org/tree-sitter-rust/-/tree-sitter-rust-0.24.0.tgz` | `4248da0b9ea40fec13ef010822e7b92a9d1ebdf74f6f2733539b1dbbb086c5bb` |
| Labeler crate | Phase 0b labeler is not implemented in this spec-only commit. Phase 0b labels are invalid unless each output artifact records the exact labeler git commit SHA. | N/A for Phase 0a |

### 13.2 Pre-committed repo list

| Role | Repo | T₀ commit SHA | Language |
|---|---|---|---|
| Pilot | `https://github.com/BurntSushi/ripgrep` (`13.0.0`; 602 commits to `master` at selection time) | `af6b6c543b224d348a8876f0c06245d9ea7929c5` | Rust |
| Held-out #1 | `https://github.com/serde-rs/serde` (`v1.0.130`; 1114 commits to `master` at selection time) | `65e1a50749938612cfbdb69b57fc4cf249f87149` | Rust |
| Held-out #2 | `https://github.com/pallets/flask` (`2.0.0`; 1300 commits to `main` at selection time) | `2f0c62f5e6e290843f03c1fa70817c7a3c7fd661` | Python |

Selection criteria for repos: substantial linear history (≥500 commits since T₀), active mutation rate, public license permitting derivative artifacts. Selection is final at freeze; substitutions require a §11 entry and re-run the leakage clock.

## 14. Freeze procedure

1. Resolve all DRAFT comments. Land final wording.
2. Fill in §13.1 (tooling pins + binary hashes), §13.2 (pilot + held-out repos with T₀ SHAs), §6.2 (model snapshot date + Phase 0c $ cap + token-price snapshot).
3. Append or update `## 15. Freeze input record` with date, signer, tooling binary hashes, model API snapshot date, token-price snapshot, and repo selection record.
4. Compute final spec hash after all file edits: `sha256sum benchmarks/provbench/SPEC.md`.
5. Record the final spec hash in the PR body and annotated `provbench-spec-v1` tag. Do not embed the final file hash inside `SPEC.md`; doing so changes the file bytes being hashed.
6. Change `Status: DRAFT, pre-freeze` at the top to `Status: FROZEN <date>` in the merge commit if the maintainer wants the status inside the hashed file, then compute and record the final hash in the tag annotation.
7. Tag the commit `provbench-spec-v1`.

Phase 0b begins after step 7 lands. Not before.

## 15. Freeze input record

- **Recorded:** 2026-05-09.
- **Signer:** Codex / Jeffrey Crum local workspace.
- **Model API snapshot:** `claude-sonnet-4-6`, snapshot date 2026-05-09.
- **Token-price snapshot:** Anthropic native API Sonnet 4.6 pricing captured on 2026-05-09: $3.00 per 1M input tokens, $15.00 per 1M output tokens.
- **Phase 0c budget cap:** $250 USD.
- **Tooling pins:** exactly the rows in §13.1.
- **Repo split:** exactly the rows in §13.2.
- **Final spec hash:** record in the PR body and `provbench-spec-v1` tag annotation after merge; it is intentionally not embedded in this file to avoid a self-referential hash.
