# ProvBench Phase 0b Labeler — Hardening Pass 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land 6 commits on `fix/provbench-labeler-hardening-pass-3` (cut from `main`, base HEAD `31136aa`) closing the 4 spot-check failure clusters identified in the first 200-row review (`benchmarks/provbench/spotcheck/2026-05-12-findings.md`, 80.50% / Wilson 74.46% — SPEC §9.1 gate FAIL). Final PR title: `fix(provbench): labeler hardening pass 3`. Post-merge: regenerate corpus + fresh spot-check → target ≥95% point estimate.

**Architecture:** TDD ordering. Task 1 lands the 5 regression tests (4 RED for the bugs, 1 preservation case GREEN). Tasks 2-5 are the GREEN fixes — visibility narrowing classified as source-changed, replay symbol resolution becomes commit-tree-local (dropping RA from the replay path), rename detection requires AST context match, DocClaim matching becomes relocation-tolerant. Task 6 is docs+CHANGELOG. No SPEC v1 edits. No fact_id schema breakage. No corpus invalidation.

**Tech Stack:** Rust 1.91 (edition 2021), `tree-sitter` 0.25.6 + `tree-sitter-rust` 0.24.0, `gix`, `similar`, `pulldown-cmark`, `serde_json`, `sha2`, `anyhow`, `tempfile`, `csv`.

## Context

This plan is the v3 batch-implementation manifest derived from the locked v1 plan (final_plan_hash recorded server-side in collab session `ac2998e2-4b95-43e2-983d-79dcd7e1ea85`). The locked plan was produced via Claude draft + Codex blind draft + canonical synthesis + Codex review (`approve_with_minor_edits`). Codex's blind draft improved on Claude's draft on every cluster:
- **A**: post-state-only visibility helper (no `Fact::PublicSymbol` schema change → existing fact_ids stay stable).
- **B**: proper commit-local `CommitSymbolIndex` + drop RA from replay entirely + AST cache per `(commit, path)`.
- **C**: typed `RenameCandidate` carrying kind + container context; explicit anti-tuning discipline.
- **D**: post-state mention search by `qualified_name`, not re-hashing at extraction (T₀ hash unchanged).
- **Ordering**: tests-first as a discrete Task 1, then implementation. More rigorous TDD.

Codex review refined Task 1: Test 4 (true-positive rename `locations_mut` → `captures_mut` in same `impl` block) likely already passes the current `similar`-based heuristic — so it's a **preservation** test (GREEN now, must stay GREEN through Task 4's tightening), not strictly RED. All technical content below is verbatim from the locked plan.

## TDD Discipline (per task)

Each subagent owns one Task. Within its task:

- [ ] **Step 1 — RED:** for fix tasks, write regression test(s) first (or rely on Task 1's tests if already landed). Run the test. Confirm it **fails** for the right reason.
- [ ] **Step 2 — GREEN:** implement the change described in `Implementation:`. Run the test. Confirm it **passes**.
- [ ] **Step 3 — Gate:** run `cargo fmt --manifest-path benchmarks/provbench/labeler/Cargo.toml --all -- --check` and `cargo clippy --manifest-path benchmarks/provbench/labeler/Cargo.toml --all-targets --all-features -- -D warnings`. Both must succeed.
- [ ] **Step 4 — Workspace test:** `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml`. Confirm all prior + new tests pass.
- [ ] **Step 5 — Commit:** `git add` only the files in this Task's `Files:` block. Commit with the **exact** commit message in the Task header. Push.

Subagents must not invoke `superpowers:finishing-a-development-branch` — the v3 collab protocol owns PR creation at the global-review stage.

---

## Branch & Base

- Base: `main` (the spot-check artifacts commit `31136aa` is the first commit on this branch).
- Branch: `fix/provbench-labeler-hardening-pass-3` (already checked out, pushed to origin with upstream tracking).
- Tasks land on top of `31136aa` in order, one commit each.

---

### Task 1 — add labeler hardening pass 3 regressions (RED + preservation)

**Commit message:** `test(provbench): add labeler hardening pass 3 regressions`

**Files:**
- New (or extend if it already exists from pass 2): `benchmarks/provbench/labeler/tests/replay_hardening.rs`
- Extend: `benchmarks/provbench/labeler/tests/diff.rs`
- Optionally: `benchmarks/provbench/labeler/tests/ast.rs` for extractor unit cases

**RED cases (must FAIL against current HEAD `31136aa`):**

1. **Visibility narrowing** — T₀ has `pub struct Config { … }`; post commit has `pub(crate) struct Config { … }`. Expected: `Label::StaleSourceChanged` for `PublicSymbol::Config`. Current actual: `Label::NeedsRevalidation`.
2. **Per-commit deletion under HEAD-similar siblings** — synthetic history: T₀ has `fn replace_with_captures()`; intermediate commit deletes it; HEAD has similarly-named `fn replace_with_caps()`. Replay at the intermediate commit must classify as `StaleSourceDeleted` based on THAT commit's tree (not HEAD-influenced).
3. **Rename false positive** — synthetic 2-commit case: T₀ has `pub struct AstAnalysis { all_verbatim_literal: bool, any_literal: bool }`; post commit drops `all_verbatim_literal`. Expected: `StaleSourceDeleted` (NOT `StaleSymbolRenamed`).
5. **DocClaim relocation** — synthetic markdown with `` `column` `` inline-code mention at line 217; post commit inserts 26 lines above (mention now at line 243, byte-identical text). Expected: `Label::Valid`.

**Preservation case (GREEN now, must stay GREEN through Task 4 — per Codex review note):**

4. **Rename true positive** — synthetic 2-commit case: `fn locations_mut()` in `impl AutomataCaptures` becomes `fn captures_mut()` in same `impl AutomataCaptures`. Expected: `Label::StaleSymbolRenamed { new_name: "captures_mut" }`. Current `similar` heuristic plausibly already handles this same-container case; Task 4's tightening must not regress it. If the test surprises us and is RED on HEAD, downgrade to RED with documented reason.

**Acceptance:**
- Tests 1, 2, 3, 5 fail against HEAD `31136aa` for the documented reasons.
- Test 4 passes against HEAD `31136aa` (preservation contract for Task 4).
- No dependency on `rust-analyzer`, network, ripgrep corpus, or wall-clock time.
- Uses `tempfile::tempdir()` + shared `tests/common/mod.rs` helpers from pass 2.

**Anti-tuning discipline:** None of these synthetic regressions are derived from the 200-row diagnostic sample. Task 4's threshold/context-check choices in particular must be justified by these synthetic regressions, NOT by hunting through `sample-2fc250a.csv` for cases that flip the agreement number.

**Commit note:** the failing tests are deliberately part of the diff. Use `--no-verify` is NOT required — Tasks 2–5 will turn them GREEN. The commit may include `#[ignore = "RED until Task N"]` annotations on the failing tests if Cargo would otherwise block the commit's `cargo test` gate. The subagent owns the choice: `#[ignore]` with an explicit annotation that gets removed as part of the corresponding GREEN task, or `#[should_panic]` with a TODO comment, or accepting a `cargo test` failure in the commit and noting it in the commit message. Pick whatever cleanly preserves the audit trail.

---

### Task 2 — classify public visibility narrowing as source changed (Cluster A, GREEN for Test 1)

**Commit message:** `fix(provbench): classify public visibility narrowing as source changed`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/facts/symbol_existence.rs`
- Modify: `benchmarks/provbench/labeler/src/replay.rs`

**Implementation:**
- Add an internal helper in `symbol_existence.rs` that finds a Rust item / re-export by qualified name **regardless of visibility** and returns its visibility state: `Bare`, `Restricted { qualifier: "crate"|"super"|"in <path>" }`, or `Private`.
- T₀ extraction unchanged: `Fact::PublicSymbol` continues to represent only bare-public symbols. **No schema change.**
- In `matching_post_fact` for `Fact::PublicSymbol`: if bare-public extraction misses, call the visibility-aware helper. If item still exists at same qualified path but no longer has bare `pub`, build a `CommitState` with `file_exists=true`, `symbol_resolves=true`, `post_span_hash` from the new item span, and `structurally_classifiable=true`. The existing first-match-wins logic in `label::classify` returns `StaleSourceChanged`.
- Cover named items (struct/enum/fn/const/etc.) AND `pub use` re-exports.

**Acceptance:**
- Test 1 passes.
- `pub` → `pub(crate)`, `pub(super)`, `pub(in path)`, and `pub` → private all classify as `StaleSourceChanged`.
- Still-public symbols remain `Valid` when hashes match.
- Truly absent symbols still classify as deleted unless a valid rename candidate is found.
- If Test 1 was `#[ignore]`-gated in Task 1's commit, remove the ignore annotation in this commit.

---

### Task 3 — resolve replay symbols from per-commit trees (Cluster B, GREEN for Test 2)

**Commit message:** `fix(provbench): resolve replay symbols from per-commit trees`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/replay.rs` (build commit-local index, remove RA from replay loop)
- Optionally a sibling module: `benchmarks/provbench/labeler/src/replay/commit_index.rs` if `replay.rs` grows beyond the project size guideline
- Modify if a trait tweak is cleaner: `benchmarks/provbench/labeler/src/resolve/mod.rs`
- Modify: `benchmarks/provbench/labeler/README.md`

**Implementation:**
- **Stop spawning `RustAnalyzer` in `Replay::run`.** Working tree must not influence per-commit classification.
- For each commit, build a `CommitSymbolIndex`:
  - Source: `git ls-tree -r <commit>` (for `.rs` and markdown) → `read_blob_at(commit, path)` per blob.
  - Per-blob: parse with tree-sitter exactly once and cache the AST under `(commit, path)` in a HashMap.
  - Answers: (a) "does the original fact still exist at its original path?"; (b) "does a same-qualified Rust symbol exist elsewhere in the commit tree?"; (c) "what same-kind candidate spans are available for rename detection?"
- **Preserve the existing blob-cache regression** (the one verifying each `(commit, path)` is read once). Update its expected count only if the new behavior legitimately changes the read pattern — never regress silently.
- **Moved-not-renamed:** if a same-qualified symbol exists elsewhere in the commit tree (different path), do NOT classify as `StaleSymbolRenamed`. Construct a state that routes to `NeedsRevalidation` via first-match-wins (legitimate gray area for the LLM to evaluate later).
- README "Behavior" section: add a sentence — replay classification is commit-tree-local; `rust-analyzer` is no longer consulted at replay time. Live RA tooling stays in the crate for `tests/replay_ra.rs` (pinned-binary test) and for future cross-crate / macro-expanded work.

**Acceptance:**
- Test 2 passes.
- `Replay::run` succeeds end-to-end against a synthetic repo with `rust-analyzer` not installed at all.
- `tests/replay_ra.rs` remains `#[ignore]`-gated and untouched.
- The existing blob-read-cache regression test still passes (or is intentionally updated with a written rationale).
- If Test 2 was `#[ignore]`-gated in Task 1, remove the ignore annotation here.

---

### Task 4 — tighten rename candidate selection (Cluster C, GREEN for Test 3; preserves Test 4)

**Commit message:** `fix(provbench): tighten rename candidate selection`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/diff/mod.rs`
- Modify: `benchmarks/provbench/labeler/src/replay.rs` (call-site updates only)
- Extend: `benchmarks/provbench/labeler/tests/diff.rs` (additional unit cases beyond Task 1's integration coverage)

**Implementation:**
- Replace the existing `(String, Vec<u8>)`-keyed candidate API with a typed `RenameCandidate { kind: FactKind, qualified_name: String, leaf_name: String, container: Option<String>, span: Vec<u8> }`.
- Filter pipeline (in order):
  1. **Same fact kind.** A `FunctionSignature` rename target must also be a function.
  2. **Compatible container** (per fact kind):
     - Field → same struct/enum variant.
     - FunctionSignature → same module/impl context where tree-sitter exposes it.
     - TestAssertion → same enclosing test function.
     - PublicSymbol → same item class.
  3. **Similarity gate.** Either (a) raise the single `f32` threshold above current `0.6`, OR (b) two-part check: normalized structural similarity (post-leaf-rename) + leaf-name evidence (small edit distance OR shared substring). **Threshold chosen from synthetic regressions in Task 1, NOT from the 200-row sample.**
- Preserve same-kind same-container true-rename behavior (Test 4 must remain GREEN).

**Acceptance:**
- Test 3 passes (false positive eliminated).
- Test 4 still passes (true positive preserved).
- New `tests/diff.rs` unit cases: sibling-overlap false positives (`all_verbatim_literal` ~ `any_literal`) return `None`; same-context renames return `Some(new_name)`.
- If Test 3 was `#[ignore]`-gated in Task 1, remove the ignore annotation here.

---

### Task 5 — make doc claims stable across line shifts (Cluster D, GREEN for Test 5)

**Commit message:** `fix(provbench): make doc claims stable across line shifts`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/facts/doc_claim.rs`
- Modify: `benchmarks/provbench/labeler/src/replay.rs`

**Implementation:**
- Add a helper in `doc_claim.rs` that scans markdown bytes and returns inline-code mention spans + hashes for a requested claim name, using the same `pulldown-cmark` parsing semantics as T₀ extraction.
- In `matching_post_fact` for `Fact::DocClaim`: do NOT reuse the original byte offset. Parse the post markdown and find an inline-code mention whose mention text matches the fact's `qualified_name`. If the mention bytes match, classification is `Valid`.
- **Duplicate-mention tie-breaker:** prefer exact-byte match first, then nearest to the original line. Never let an unrelated mention at the old offset drive the result.
- Invalid UTF-8 in markdown continues to fail closed with `parse <path> @ <sha>` context (pass-2 behavior preserved).
- **T₀ extraction is unchanged** — no hash change → existing fact_ids stable.

**Acceptance:**
- Test 5 passes.
- Inserting markdown above a surviving mention keeps the DocClaim `Valid`.
- Removing the mention entirely no longer produces a false `Valid` by hashing unrelated bytes at the old offset.
- Existing invalid-UTF-8 doc-extractor tests (from pass 2) still pass.
- If Test 5 was `#[ignore]`-gated in Task 1, remove the ignore annotation here.

---

### Task 6 — document labeler hardening pass 3

**Commit message:** `docs(provbench): document labeler hardening pass 3`

**Files:**
- Modify: `benchmarks/provbench/labeler/README.md` (Behavior section)
- Modify: `CHANGELOG.md` (repo root)

**Implementation:**
- README "Behavior" section gains four sub-bullets:
  - "Visibility narrowing classified as source changed." Brief explanation tying back to SPEC §5.
  - "Replay symbol resolution is commit-tree-local." Mentions tree-sitter + deliberate removal of runtime RA dependency. Notes RA tooling pin and `tests/replay_ra.rs` still exist for future cross-crate / macro-expanded work.
  - "Rename detection requires AST context match." Brief.
  - "DocClaim matching is relocation-tolerant." Brief.
- `CHANGELOG.md` entry under `[Unreleased]` (or whatever pattern pass 2 established), dated `2026-05-12`.
- **Do NOT touch `benchmarks/provbench/SPEC.md`** — frozen.

**Acceptance:**
- `cargo doc --no-deps --manifest-path benchmarks/provbench/labeler/Cargo.toml` clean.
- All previously RED tests now pass.

---

## Pre-PR Verification (run after Task 6; gates the global-review handoff)

1. `cargo fmt --manifest-path benchmarks/provbench/labeler/Cargo.toml --all -- --check`
2. `cargo clippy --manifest-path benchmarks/provbench/labeler/Cargo.toml --all-targets --all-features -- -D warnings`
3. `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml`
4. `cargo test --workspace`
5. `cargo doc --no-deps --manifest-path benchmarks/provbench/labeler/Cargo.toml`

## Out of Scope (explicit)

- **Corpus regeneration + fresh 200-row spot-check.** Post-merge validation, not part of this PR.
- **Per-commit RA workspace.** The deeper PR #30 TODO is sidestepped by making tree-sitter the per-commit resolver of record; live RA tooling stays in the crate for future tasks.
- **SPEC v1 changes** (frozen at tag `provbench-spec-v1`, sha256 `683d023…`).
- **`PostCommitState` redundancy refactor**, **legacy `function_signature::iter` removal**, **macOS-x86_64 / aarch64-linux tooling pins** (all deferred from pass 2).

## Codex Review Refinements Incorporated

1. **Task 1**: Test 4 (true-positive rename) reframed as a **preservation case** (GREEN now, must stay GREEN through Task 4's tightening) rather than strictly RED. If empirically it turns out to be RED on HEAD, downgrade with written rationale.
2. **Anti-tuning language preserved.** Synthetic regressions are the tuning target; the 200-row diagnostic sample is evidence, not training data.

## Critical Files Referenced

- `benchmarks/provbench/labeler/src/facts/symbol_existence.rs` (visibility-aware helper — Task 2)
- `benchmarks/provbench/labeler/src/facts/doc_claim.rs` (post-state mention search — Task 5)
- `benchmarks/provbench/labeler/src/diff/mod.rs` (typed `RenameCandidate` — Task 4)
- `benchmarks/provbench/labeler/src/replay.rs` (commit-local index, RA removal, all match-site updates — Tasks 2/3/4/5)
- `benchmarks/provbench/labeler/src/resolve/mod.rs`, `src/resolve/rust_analyzer.rs` (gating only — Task 3)
- `benchmarks/provbench/labeler/src/output.rs` (unchanged — no schema change)
- `benchmarks/provbench/labeler/src/label.rs` (rule ordering preserved per SPEC §5 — no changes)
- `benchmarks/provbench/labeler/tests/replay_hardening.rs`, `tests/diff.rs`, `tests/ast.rs` (Task 1)
- `benchmarks/provbench/labeler/README.md`, repo-root `CHANGELOG.md` (Task 6)
- Reference data (committed at HEAD `31136aa`): `benchmarks/provbench/spotcheck/sample-2fc250a.csv`, `2026-05-12-findings.md`
