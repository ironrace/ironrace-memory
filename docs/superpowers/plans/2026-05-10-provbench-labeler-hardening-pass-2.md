# ProvBench Phase 0b Labeler — Hardening Pass 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land 9 commits on `fix/provbench-labeler-hardening-pass-2` (cut from `main@920da5a`) closing the genuine correctness, robustness, and CI-portability gaps identified in IronRace collab session `bb22d79c-f1fa-429a-b03e-29b3c0ea551c`. Final PR title: `fix(provbench): labeler hardening pass 2`.

**Architecture:** One commit per Task; each Task is a self-contained TDD cycle (write failing regression test → fix → confirm green → commit). All work is scoped to `benchmarks/provbench/labeler/` plus repo-root `CHANGELOG.md`. No SPEC v1 changes (frozen at tag `provbench-spec-v1`, sha256 `683d023…`).

**Tech Stack:** Rust 1.91 (edition 2021), `tree-sitter` 0.25.6, `rust-analyzer` 1.85.0 (LSP stdio), `gix`, `similar`, `pulldown-cmark`, `serde_json`, `sha2`, `clap`, `anyhow`, `thiserror`, `tracing`. New dep: `csv` (production).

## Context

This plan is the v3 batch-implementation manifest derived from the locked v1 plan (final_plan_hash recorded server-side in collab session `bb22d79c`). The locked plan was produced via Claude draft + Codex blind draft + canonical synthesis + Codex review (`approve_with_minor_edits`). Codex's blind draft caught four real bugs Claude's first-pass triage had marked "already addressed" — UTF-8 corruption in percent decoder, RA timeout returning `Ok` on fail, silent invalid-UTF-8 swallowing in doc extractor, test-assertion traversal missing the first asserted symbol. All technical content below is verbatim from the locked plan.

## TDD Discipline (per task)

Each subagent owns one Task. Within its task:

- [ ] **Step 1 — RED:** write a regression test that captures the acceptance bullets. Run it. Confirm it **fails** for the right reason.
- [ ] **Step 2 — GREEN:** implement the change described in `What:`. Run the test. Confirm it **passes**.
- [ ] **Step 3 — Gate:** run `cargo fmt --manifest-path benchmarks/provbench/labeler/Cargo.toml --all -- --check` and `cargo clippy --manifest-path benchmarks/provbench/labeler/Cargo.toml --all-targets --all-features -- -D warnings`. Both must succeed.
- [ ] **Step 4 — Workspace test:** `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml`. Confirm all prior + new tests pass.
- [ ] **Step 5 — Commit:** `git add` only the files in this Task's `Files:` block and any test file added in Step 1. Commit with the **exact** commit message in the Task header. Push.

Subagents must not invoke `superpowers:finishing-a-development-branch` — the v3 collab protocol owns PR creation at the global-review stage.

---

## Branch & Base

- Base: `main@920da5a` (PR #30 merge commit).
- Branch: `fix/provbench-labeler-hardening-pass-2`.
- Already created and checked out at the start of this v3 phase.

---

### Task 1: canonicalize repo root + repo-relative fact paths (#1+#7)

**Commit message:** `fix(provbench): canonicalize repo root + repo-relative fact paths`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/repo.rs` (`Pilot::open`)
- Modify: `benchmarks/provbench/labeler/src/replay.rs:242-305`
- Reuse: `benchmarks/provbench/labeler/src/resolve/rust_analyzer.rs:82` (already-canonicalized workspace root — keep, reuse)
- Test: `benchmarks/provbench/labeler/tests/replay.rs` (or new `tests/path_canon.rs`)

**What:**
- In `repo::open` / `Pilot::open`: call `Path::canonicalize` **once** on the user-supplied repo root, store the result on the `Pilot`. This is the only filesystem-resolving canonicalization in this commit. (Codex review note 3.)
- Add a pure helper `canonicalize_for_fact_id(rel_git_path: &Path) -> String` that:
  - Treats input as a **repo-relative git tree path** (the kind that comes out of `git ls-tree`),
  - Replaces `\\` with `/`,
  - Returns a `String`.
- Helper performs **no filesystem I/O**, so T₀ replay determinism is preserved for moved/deleted files. (Codex review note 3.)
- Apply at every `fact_id` derivation site (currently the `source_path.display()` / `doc_path.display()` calls).
- No absolute filesystem path may appear in any emitted `fact_id`.

**Acceptance:**
- New unit test: `fact_id` byte-identical when labeler runs on the same `t0_sha` from two different `pwd`s on the same filesystem.
- New unit test: paths with spaces and UTF-8 segments round-trip through `canonicalize_for_fact_id` without filesystem access.
- New regex assertion in test: no `/Users/`, `/home/`, or absolute Windows path appears in any emitted `fact_id`.
- Existing `tests/determinism.rs` still passes.

---

### Task 2: UTF-8-safe percent encoding/decoding for file URIs (#4)

**Commit message:** `fix(provbench): UTF-8-safe percent encoding/decoding for file URIs`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/resolve/rust_analyzer.rs:419-445` (`uri_to_path` and any matching encoder)
- Test: `benchmarks/provbench/labeler/tests/rust_analyzer.rs`

**What:**
- Current decoder maps each decoded byte to `char` individually — corrupts multi-byte UTF-8 sequences (`%E2%94%80` → garbage instead of `─`).
- Rewrite to accumulate decoded bytes into `Vec<u8>`, then `String::from_utf8(bytes).context("invalid UTF-8 in percent-decoded URI")?`. Return `Result`.
- Mirror change to the encoder if it makes the same byte-vs-char error.
- Optional: replace with the `percent-encoding` crate if simpler than the hand-rolled fix; this is an implementer call.

**Acceptance:**
- New unit test: `file:///tmp/%E2%94%80%E2%94%80/foo.rs` round-trips to `──/foo.rs`.
- New unit test: `%20`-bearing paths still work.
- Existing tests pass.

---

### Task 3: rust-analyzer indexing timeout fails closed (#2)

**Commit message:** `fix(provbench): rust-analyzer indexing timeout fails closed`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/resolve/rust_analyzer.rs:146-189` (`wait_for_indexing`)
- Test: `benchmarks/provbench/labeler/tests/rust_analyzer.rs`

**What:**
- Track whether RA emitted at least one `$/progress` `begin` notification.
- **Begun-but-never-ended before deadline** → return `Err(anyhow!("rust-analyzer indexing timed out at {workspace_root}"))`. Do **not** return `Ok(())` with a `warn!` (current behavior).
- **No progress observed** (small workspace, instant indexing) → keep current quiet-period success path.
- Plumb the error up so replay surfaces it per-commit instead of producing silent zero-symbol resolution.

**Acceptance:**
- New unit test (synthetic LSP messages, no RA spawn): begun-without-end progress before deadline → `Err`.
- New unit test: zero progress messages within quiet period → `Ok(())`.
- Existing `tests/replay_ra.rs` still gated on the binary hash.

---

### Task 4: explicit invalid-UTF-8 handling in doc extractor (#3)

**Commit message:** `fix(provbench): explicit invalid-UTF-8 handling in doc extractor`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/facts/doc_claim.rs:80`
- Modify: `benchmarks/provbench/labeler/src/replay.rs` (call site)
- Test: `benchmarks/provbench/labeler/tests/replay.rs` (or co-locate in a doc-claim-specific test file)

**What:**
- Replace `from_utf8(...).unwrap_or_default()` with `from_utf8(...)?` (lifted to `Result`). Extractor signature becomes `fn extract(...) -> Result<Vec<Fact>>`.
- Replay annotates the error with `parse <path> @ <sha>` so reviewers can find the offending blob.

**Acceptance:**
- New unit test: invalid UTF-8 bytes in a README produce a contextual error, not silent zero facts.
- Existing extractor tests pass.

---

### Task 5: structured CSV via csv crate (#5)

**Commit message:** `fix(provbench): structured CSV via csv crate`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/spotcheck.rs:96-101` (writer) and report-parsing call site
- Modify: `benchmarks/provbench/labeler/Cargo.toml` (add `csv` to `[dependencies]`)
- Test: `benchmarks/provbench/labeler/tests/spotcheck.rs`

**What:**
- `csv` is added to `[dependencies]`, **not** `[dev-dependencies]`. (Codex review note 2 — spot-check writing and report parsing run in production CLI paths, so the dep is production-class.)
- Replace the hand-rolled writer with `csv::Writer`. Replace any line-based report reader with `csv::Reader`.
- Preserve column order: `fact_id,commit_sha,bucket,predicted_label,human_label,disagreement_notes`.

**Acceptance:**
- New unit test: round-trip of a row containing `,`, `"`, `\n`, and `\r` in `disagreement_notes`.
- New unit test: report parser handles a reviewer note with a quoted newline without column drift.
- Existing spot-check tests pass.

---

### Task 6: consistent test-assertion macro/symbol traversal (#10)

**Commit message:** `fix(provbench): consistent test-assertion macro/symbol traversal`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/facts/test_assertion.rs` (traversal logic)
- Test: `benchmarks/provbench/labeler/tests/ast.rs` (regression test)

**What:**
- First add a regression test reproducing the missed-first-symbol case (e.g. `assert_eq!(some_func(x), expected)` where `some_func` is the intended first asserted symbol). Test must fail against current code.
- Then unify the macro-name and asserted-symbol traversal to use the same direct-child / named-child iteration pattern (the one used in `function_signature.rs`).

**Acceptance:**
- Regression test fails before the fix, passes after.
- All existing `tests/ast.rs` tests pass.

---

### Task 7: pin tooling for linux-x86_64 (ubuntu-latest CI) (#11)

**Commit message:** `feat(provbench): pin tooling for linux-x86_64 (ubuntu-latest CI)`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/tooling.rs:18-67`
- Modify: `benchmarks/provbench/labeler/README.md`
- Test: `benchmarks/provbench/labeler/tests/tooling.rs`

**What:**
- Refactor pinned-hash storage from a flat const to a `&[(target_os, target_arch, &BinaryName, Sha256, FallbackPath)]` slice.
- Pin **exact** versions: rust-analyzer **1.85.0** and tree-sitter **0.25.6** (Codex review note 1 — keep the existing 0.25.6, do not silently bump). Both for `x86_64-unknown-linux-gnu`.
- Record artifact provenance in code comments above the entries: artifact URLs (`https://github.com/rust-lang/rust-analyzer/releases/download/2024-…/rust-analyzer-x86_64-unknown-linux-gnu.gz` and the tree-sitter equivalent) and the published sha256 verbatim. **Implementer must verify hashes against the live release page before committing** — do not invent or copy from any other source.
- `resolve_from_env()` selects the row matching the running `(target_os, target_arch)`; hard-fails with a clear error listing supported platforms.
- README "Reproducibility" section: supported platforms = `aarch64-darwin`, `x86_64-linux-gnu`. Other platforms explicitly out-of-scope for this PR.

**Acceptance:**
- `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml` passes on `ubuntu-latest` CI runner.
- `cargo test --workspace` still green on macOS arm64.
- Resolving on `aarch64-linux` or `x86_64-darwin` fails with the documented error.

---

### Task 8: targeted replay regressions for hardening surface (#9)

**Commit message:** `test(provbench): targeted replay regressions for hardening surface`

**Files:**
- Test: `benchmarks/provbench/labeler/tests/replay.rs` (or new `tests/replay_hardening.rs`)
- Test: `benchmarks/provbench/labeler/tests/spotcheck.rs`

**What (targeted regressions only — no broad fixture churn):**
- Replay regression: deterministic fact-id under different `pwd` (consumes Task 1).
- Replay regression: invalid-UTF-8 README → contextual error (consumes Task 4).
- Replay regression: at least one non-function fact-type label transition (changed/deleted) across commits.
- CSV round-trip regression with reviewer-note edge cases (consumes Task 5; can live in `tests/spotcheck.rs`).

**Acceptance:**
- All new tests pass.
- No existing test broken.
- No new `#[ignore]`-gated tests beyond the existing pinned-RA gate.

---

### Task 9: rustdoc + README + CHANGELOG (#8 + #12 annotate-only)

**Commit message:** `docs(provbench): rustdoc + README + CHANGELOG`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/lib.rs`
- Modify: `benchmarks/provbench/labeler/src/replay.rs`
- Modify: `benchmarks/provbench/labeler/src/output.rs` (`FactAtCommit`)
- Modify: `benchmarks/provbench/labeler/src/facts/*.rs`
- Modify: `benchmarks/provbench/labeler/README.md`
- Modify: `CHANGELOG.md` (repo-root)

**What:**
- `//!` module-level docs on `lib.rs` and each public module: one paragraph per module describing the SPEC role.
- `///` rustdoc on `ReplayConfig`, `Replay::run`, `FactAtCommit`, `labeler_stamp`, the public extractor entry-points.
- `#[doc(hidden)]` (or doc-only deprecation note) on the legacy `function_signature::iter` shim (handles #12 without removal).
- README update: clarify platform support (Task 7) and the new fail-closed/explicit-error behaviors (Tasks 2, 3, 4).
- Root `CHANGELOG.md` entry: `"ProvBench labeler — Phase 0b hardening pass 2 (deterministic paths, fail-closed RA timeout, UTF-8-safe URI handling, structured CSV, linux-x86_64 CI)."` (Create the file if absent.)

**Acceptance:**
- `cargo doc --no-deps --manifest-path benchmarks/provbench/labeler/Cargo.toml` builds without warnings.
- Existing tests pass.

---

## Pre-PR Verification (run after Task 9; gates the global-review handoff)

1. `cargo fmt --manifest-path benchmarks/provbench/labeler/Cargo.toml --all -- --check`
2. `cargo clippy --manifest-path benchmarks/provbench/labeler/Cargo.toml --all-targets --all-features -- -D warnings`
3. `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml` (≥ 50 passing; 2 ignored)
4. `cargo test --workspace`
5. `cargo doc --no-deps --manifest-path benchmarks/provbench/labeler/Cargo.toml`
6. After push: GitHub Actions `Test (ubuntu-latest)` runs labeler tests successfully (was previously blocked by single-platform pin).
7. Manual: re-run labeler on the ripgrep pilot from two different `pwd`s; diff JSONL — byte-identical.

## Out of Scope (explicit)

- SPEC v1 changes (frozen at tag `provbench-spec-v1`, sha256 `683d023…`).
- macOS x86_64 / aarch64-linux tooling pins (separate PR; needs genuine published hashes).
- Per-commit worktree synchronization for rust-analyzer (separate, larger work).
- `PostCommitState` redundancy refactor (#6 — deferred).
- Removal of legacy `function_signature::iter` (#12 — annotate-only here).
- Anything in `crates/ironmem/`.
- Phase 0c LLM baseline.

## Codex Review Refinements (already incorporated above)

1. **Task 7**: pinned exact `tree-sitter 0.25.6` and `rust-analyzer 1.85.0`; record artifact URLs and verbatim published sha256 (no synthesis).
2. **Task 5**: `csv` crate added to `[dependencies]` (production), not `[dev-dependencies]`.
3. **Task 1**: helper operates only on repo-relative git tree paths and does no filesystem I/O; only `Pilot::open` does a single `Path::canonicalize` of the repo root.

## Critical Files Referenced

- `benchmarks/provbench/labeler/src/repo.rs` (`Pilot::open` — single canonicalize site)
- `benchmarks/provbench/labeler/src/replay.rs` (paths in fact_id, replay loop)
- `benchmarks/provbench/labeler/src/resolve/rust_analyzer.rs` (timeout, percent decoding)
- `benchmarks/provbench/labeler/src/facts/doc_claim.rs` (UTF-8 handling)
- `benchmarks/provbench/labeler/src/facts/test_assertion.rs` (traversal)
- `benchmarks/provbench/labeler/src/spotcheck.rs` (CSV writer/reader)
- `benchmarks/provbench/labeler/src/tooling.rs` (platform pins)
- `benchmarks/provbench/labeler/src/lib.rs` (rustdoc surface)
- `benchmarks/provbench/labeler/Cargo.toml` (add `csv` to `[dependencies]`)
- `benchmarks/provbench/labeler/tests/replay.rs`, `tests/replay_ra.rs`, `tests/ast.rs`, `tests/spotcheck.rs`, `tests/rust_analyzer.rs`, `tests/tooling.rs`
- `benchmarks/provbench/labeler/README.md` (platform support, behavior changes)
- `CHANGELOG.md` (repo-root, new entry)
