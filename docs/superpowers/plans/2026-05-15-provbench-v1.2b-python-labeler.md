# ProvBench v1.2b — Python Labeler Bring-up Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the existing Rust `provbench-labeler` crate so it can deterministically label Python repos using the same `fact_at_commit` schema as the Rust path. Acceptance is the SPEC §9.1 gate (≥95% Wilson lower bound on a 200-sample spot-check) applied to `pallets/flask @ 2f0c62f5e6e290843f03c1fa70817c7a3c7fd661` plus byte-identical two-run determinism on the full flask corpus.

**Architecture:** Pure-Rust extension to `benchmarks/provbench/labeler/`. Add `tree-sitter-python` Rust crate as a sibling of `tree-sitter-rust`. New sibling modules `ast/python.rs` and `resolve/python.rs` implement the language-specific surface; the existing `SymbolResolver` trait and AST module comments already anticipate this (`resolve/mod.rs:3` and `ast/mod.rs:2`). Replay (`replay/mod.rs:610`) currently filters `.rs` files only — generalize to dispatch by file extension via a `Language` enum. Per-fact extractors under `facts/` get Python siblings that emit the same `Fact` variants as the Rust path. Symbol resolution is tree-sitter scope chains + a hand-rolled lexical import graph (no external Python runtime); the existing `rust_analyzer` resolver is unchanged.

**Tech stack:** Rust 1.91 (edition 2021), `tree-sitter = "0.25"`, `tree-sitter-python = "0.25"`, `tree-sitter-rust = "0.24"` (unchanged), `gix`, `similar`, `pulldown-cmark`, `sha2`, `clap`, `anyhow`, `thiserror`, `tracing`, `tempfile` (dev).

**Non-goals:**
- Held-out evaluation on flask (that's Plan B: `2026-05-15-provbench-v1.2b-flask-heldout.md`).
- Rule-set retuning. Phase 1 rules (v1.2, phase1 SHA `97cef97`) are byte-frozen for held-out use; this plan does not touch `phase1/`.
- Python runtime support inside ironmem. The labeler stays Rust-only; flask is parsed via the `tree-sitter-python` Rust crate (vendored C grammar). No `.py` source ships in this repo.
- Cross-file Python type inference, dynamic dispatch resolution, star-import resolution, runtime metaclass effects. Documented as known limitations in §9.1 spot-check + findings.

**Frozen anchors (do not bump in this plan):**
- Spec freeze hash: `683d023934c181a8714b9d24c53d011caed31f511becf82ed9e5def92e0ff37c`
- SPEC §13.1 grammar pin: `tree-sitter-python@0.25.0`, npm tarball SHA-256 `63b76b3fa8181fd79eaad4abcdb21e2babcb504dbfc7710a89934fa456d26096`
- SPEC §13.2 held-out #2: `pallets/flask @ 2f0c62f5e6e290843f03c1fa70817c7a3c7fd661`, T₀ tag `2.0.0`
- Existing labeler git SHA (Rust-only baseline): `c2d3b7b03a51a9047ff2d50077200bb52f149448` (corpus runs) / `ababb37` (`emit-facts` / `emit-diffs` subcommands)

**Branch routing:** Execute this plan on a fresh branch cut from `main`, e.g. `feat/provbench-v1.2b-python-labeler`. Plan B (`feat/provbench-v1.2b-flask-heldout`) consumes this plan's merged result. Do NOT execute Plan A on the existing flask-heldout branch — Plan B pins this plan's merged SHA, and a fork inside the same branch defeats that pin.

**Acceptance gates (all required):**
1. Existing Rust labeler canary on ripgrep: byte-stable corpus + facts + diffs across `c2d3b7b0` and this plan's HEAD (no Rust-path regression).
2. New `tests/determinism_python.rs`: two consecutive runs on a small in-repo Python fixture (`labeler/tests/data/python/`) produce byte-identical corpus + facts + diffs.
3. New `tests/determinism_flask.rs` (`#[ignore]`, opt-in via `cargo test -- --ignored`): two consecutive runs of `run` + `emit-facts` + `emit-diffs` on `work/flask` produce byte-identical artifacts.
4. SPEC §9.1 spot-check on flask: 200-sample CSV scored by hand, Wilson 95% lower bound ≥ 0.95. Sampler reuses the existing `provbench-labeler spotcheck` subcommand with `--lang=python`.
5. `cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean.
6. Existing Rust spot-check + hardening passes 2-5 retest: re-run the existing `cli_spotcheck.rs` / `replay_hardening.rs` test suites without modification — they must remain green.

**On §9.1 spot-check miss:** STOP. The mechanical Python labeler is the load-bearing input to Plan B — if it disagrees with human review on >5% of samples we either tighten the extractors or drop the affected fact kind for Python. Do not weaken the spot-check threshold to make this pass; that would be a SPEC §10 leakage.

---

## File Structure

| Path | Responsibility | New / Modified |
|---|---|---|
| `benchmarks/provbench/labeler/Cargo.toml` | Add `tree-sitter-python = "0.25"` to `[dependencies]` | Modified |
| `benchmarks/provbench/labeler/src/lang.rs` | New `Language` enum + per-extension dispatch helper | New |
| `benchmarks/provbench/labeler/src/lib.rs` | `pub mod lang;` | Modified |
| `benchmarks/provbench/labeler/src/ast/mod.rs` | Re-export `python::PythonAst` alongside `RustAst` | Modified |
| `benchmarks/provbench/labeler/src/ast/python.rs` | tree-sitter-python parser wrapper, mirror of `RustAst` | New |
| `benchmarks/provbench/labeler/src/ast/spans.rs` | Unchanged (Span struct is language-agnostic) | — |
| `benchmarks/provbench/labeler/src/facts/mod.rs` | Add `python::` submodule re-exports per fact kind | Modified |
| `benchmarks/provbench/labeler/src/facts/python/mod.rs` | Python fact dispatch | New |
| `benchmarks/provbench/labeler/src/facts/python/function_signature.rs` | Python `def` / async def signature extractor | New |
| `benchmarks/provbench/labeler/src/facts/python/field.rs` | Python class attribute + dataclass-field extractor | New |
| `benchmarks/provbench/labeler/src/facts/python/symbol_existence.rs` | Python module-level binding extractor | New |
| `benchmarks/provbench/labeler/src/facts/python/doc_claim.rs` | Python docstring + README claim extractor (pulldown-cmark for `.md`; `ast::python` for module/class/function docstrings) | New |
| `benchmarks/provbench/labeler/src/facts/python/test_assertion.rs` | pytest assertion + unittest assertion extractor | New |
| `benchmarks/provbench/labeler/src/resolve/mod.rs` | `pub mod python;` (trait unchanged) | Modified |
| `benchmarks/provbench/labeler/src/resolve/python.rs` | Tree-sitter scope chains + lexical import graph implementing `SymbolResolver` | New |
| `benchmarks/provbench/labeler/src/replay/mod.rs` | Replace `.rs`-only file filter with `Language::for_path` dispatch; multi-language source set | Modified |
| `benchmarks/provbench/labeler/src/replay/commit_index.rs` | Generalize commit-local symbol index to handle Python | Modified |
| `benchmarks/provbench/labeler/src/replay/match_post.rs` | Generalize Myers-diff renamer to Python (file-stem heuristic, no language-specific behavior change for Rust) | Modified |
| `benchmarks/provbench/labeler/src/spotcheck.rs` | Accept `--lang={rust,python}`; sampler stratifies by language when both are present | Modified |
| `benchmarks/provbench/labeler/src/main.rs` | Plumb `--lang` through CLI for `spotcheck`; corpus emit auto-detects | Modified |
| `benchmarks/provbench/labeler/tests/data/python/repo/` | Tiny fixture repo (~6 files) used by Python AST + resolve + replay tests | New |
| `benchmarks/provbench/labeler/tests/python_ast.rs` | Unit tests for `PythonAst` span extraction | New |
| `benchmarks/provbench/labeler/tests/python_resolve.rs` | Unit tests for `resolve::python::PythonResolver` | New |
| `benchmarks/provbench/labeler/tests/python_facts.rs` | Per-fact-kind extraction tests on the fixture | New |
| `benchmarks/provbench/labeler/tests/determinism_python.rs` | Fixture-level two-run byte-identical test | New |
| `benchmarks/provbench/labeler/tests/determinism_flask.rs` | `#[ignore]` full-flask two-run byte-identical test | New |
| `benchmarks/provbench/labeler/tests/cli_spotcheck_python.rs` | CLI test for `spotcheck --lang=python` | New |
| `benchmarks/provbench/labeler/tests/common/mod.rs` | Add `python_fixture_repo()` helper | Modified |
| `benchmarks/provbench/labeler/README.md` | Document Python usage + grammar pin | Modified |
| `.gitignore` | `benchmarks/provbench/work/flask` (if not already covered by `work/` rule) | Possibly modified |
| `benchmarks/provbench/SPEC.md` | NO entry from Plan A — labeler-pin entry is recorded by Plan B in its §11 row | — |

**Module boundaries:** Each Python file ≤400 lines. If `resolve/python.rs` exceeds 400 lines split as `resolve/python/mod.rs` + `resolve/python/import_graph.rs` + `resolve/python/scope.rs`. Same rule for `facts/python/*` — split if any single fact extractor exceeds 400 lines.

---

## Task 1: Pre-flight + branch + baseline canary

**Files:**
- Inspect: `benchmarks/provbench/labeler/Cargo.toml`, `benchmarks/provbench/labeler/src/replay/mod.rs:606-617`, `benchmarks/provbench/labeler/src/ast/mod.rs`, `benchmarks/provbench/labeler/src/resolve/mod.rs`
- Use: `benchmarks/provbench/results/phase1/2026-05-14-canary/` (existing v1.0 baseline artifacts for comparison)

- [ ] **Step 1: Cut Plan A branch off `main`**

```bash
git fetch origin main
git checkout main && git pull --ff-only origin main
git checkout -b feat/provbench-v1.2b-python-labeler
git rev-parse --abbrev-ref HEAD   # expect: feat/provbench-v1.2b-python-labeler
```

Expected: branch created off the latest `main` (post-v1.2a-merge).

- [ ] **Step 2: Capture the pre-change Rust canary baseline**

The labeler is in the workspace-excluded crate at `benchmarks/provbench/labeler/`; build and run from inside that directory OR via the manifest path. The subcommand is `run` (not `emit-corpus`). Every corpus row contains a `labeler_git_sha` field stamped from the labeler crate's build-time git SHA, so a raw `sha256sum` of the JSONL would change every commit even when the per-row content is byte-identical — strip that field before hashing.

```bash
cd benchmarks/provbench/labeler && cargo build --release && cd -
mkdir -p /tmp/canary-pre
benchmarks/provbench/labeler/target/release/provbench-labeler run \
  --repo benchmarks/provbench/work/ripgrep \
  --t0   af6b6c543b224d348a8876f0c06245d9ea7929c5 \
  --out  /tmp/canary-pre/corpus.jsonl
jq -c 'del(.labeler_git_sha)' /tmp/canary-pre/corpus.jsonl \
  | sha256sum | cut -d' ' -f1 \
  > docs/superpowers/plans/work/2026-05-15-python-labeler-canary-pre.txt
cat docs/superpowers/plans/work/2026-05-15-python-labeler-canary-pre.txt
```

Expected: a 64-char hex SHA-256 you record in `docs/superpowers/plans/work/2026-05-15-python-labeler-canary-pre.txt`. This is the byte-identity anchor; Task 18 re-runs the same strip-then-hash and asserts the SHA matches.

If `work/ripgrep` is not present, clone it first:
```bash
git clone https://github.com/BurntSushi/ripgrep benchmarks/provbench/work/ripgrep
git -C benchmarks/provbench/work/ripgrep checkout af6b6c543b224d348a8876f0c06245d9ea7929c5
```

- [ ] **Step 3: Run all existing labeler tests green before any change**

```bash
cargo test -p provbench-labeler --release
```

Expected: PASS. If anything fails on `main`, STOP and report — this plan assumes a green baseline.

- [ ] **Step 4: Commit the baseline-canary SHA file**

```bash
mkdir -p docs/superpowers/plans/work
# baseline SHA captured above
git add docs/superpowers/plans/work/2026-05-15-python-labeler-canary-pre.txt
git commit -m "chore(provbench): record pre-change Rust canary SHA for Python labeler bring-up baseline"
```

---

## Task 2: Introduce `Language` enum (no-op refactor)

**Files:**
- Create: `benchmarks/provbench/labeler/src/lang.rs`
- Modify: `benchmarks/provbench/labeler/src/lib.rs`
- Test: `benchmarks/provbench/labeler/tests/lang.rs`

- [ ] **Step 1: Write the failing test**

```rust
// benchmarks/provbench/labeler/tests/lang.rs
use provbench_labeler::lang::Language;
use std::path::Path;

#[test]
fn for_path_rust() {
    assert_eq!(Language::for_path(Path::new("src/lib.rs")), Some(Language::Rust));
}

#[test]
fn for_path_python() {
    assert_eq!(Language::for_path(Path::new("src/app.py")), Some(Language::Python));
}

#[test]
fn for_path_markdown_is_none() {
    // Markdown is handled separately by doc-claim extractors, not Language.
    assert_eq!(Language::for_path(Path::new("README.md")), None);
}

#[test]
fn for_path_unknown_extension() {
    assert_eq!(Language::for_path(Path::new("data.toml")), None);
    assert_eq!(Language::for_path(Path::new("Makefile")), None);
}

#[test]
fn source_extensions_is_stable_sorted() {
    let exts = Language::source_extensions();
    let mut owned: Vec<&str> = exts.to_vec();
    owned.sort();
    assert_eq!(owned, exts);
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p provbench-labeler --test lang
```

Expected: FAIL (`use of undeclared module 'lang'`).

- [ ] **Step 3: Implement `Language`**

```rust
// benchmarks/provbench/labeler/src/lang.rs
//! Language enum + per-path dispatch. Stable extension order so any
//! iteration over `Language::source_extensions()` is deterministic.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
}

impl Language {
    /// Detect language from a path's extension. Returns `None` for paths
    /// that are not source files (e.g., `.md`, `.toml`).
    pub fn for_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Some(Language::Rust),
            Some("py") => Some(Language::Python),
            _ => None,
        }
    }

    /// Stable lexicographic order. Replay iterates this list to
    /// build a deterministic per-language file partition.
    pub fn source_extensions() -> &'static [&'static str] {
        &["py", "rs"]
    }

    pub fn extension(self) -> &'static str {
        match self {
            Language::Rust => "rs",
            Language::Python => "py",
        }
    }
}
```

- [ ] **Step 4: Wire it into `lib.rs`**

Edit `benchmarks/provbench/labeler/src/lib.rs` — add `pub mod lang;` alongside the existing `pub mod` declarations.

- [ ] **Step 5: Run the test and the full suite**

```bash
cargo test -p provbench-labeler --test lang
cargo test -p provbench-labeler --release
```

Expected: new test PASS, full suite still PASS (no regression — `Language` is unused yet).

- [ ] **Step 6: Commit**

```bash
git add benchmarks/provbench/labeler/src/lang.rs \
        benchmarks/provbench/labeler/src/lib.rs \
        benchmarks/provbench/labeler/tests/lang.rs
git commit -m "feat(provbench-labeler): add Language enum + per-path dispatch"
```

---

## Task 3: Replace `.rs`-only file filter with `Language`-based filter

**Files:**
- Modify: `benchmarks/provbench/labeler/src/replay/mod.rs` (functions at lines ~606-617 currently filter by extension `rs`)
- Test: `benchmarks/provbench/labeler/tests/replay.rs` (existing tests must still pass byte-for-byte)

- [ ] **Step 1: Read the current source-file walker**

Open `benchmarks/provbench/labeler/src/replay/mod.rs` around line 606 (`fn ... all .rs file paths ...`). Note its signature and call sites.

- [ ] **Step 2: Write a failing characterization test for behavioural equivalence on Rust-only trees**

```rust
// benchmarks/provbench/labeler/tests/replay.rs  (append)
#[test]
fn source_files_rust_only_unchanged() {
    let repo = common::rust_fixture_repo();
    let before = list_source_files_legacy(&repo); // helper that hard-codes "rs"
    let after  = list_source_files(&repo);        // new generic walker
    assert_eq!(before, after);
}
```

(`list_source_files_legacy` is a private copy of the current `.rs`-only walker kept in the test module only; delete it once the test passes.)

- [ ] **Step 3: Run test to verify it fails (or compiles only if `list_source_files` is exposed)**

```bash
cargo test -p provbench-labeler --test replay source_files_rust_only_unchanged
```

Expected: FAIL — `list_source_files` is not yet pub-visible OR returns a different set.

- [ ] **Step 4: Replace the filter (Rust → Language dispatch)**

```rust
// benchmarks/provbench/labeler/src/replay/mod.rs
use crate::lang::Language;

/// All source-file paths present in the git tree at `sha`, partitioned
/// implicitly by `Language::for_path`. Order is identical to the legacy
/// `.rs`-only walker for Rust-only trees (stable git tree-order, then
/// `.rs` < anything-else-by-Language::source_extensions, but Rust trees
/// produce zero non-`.rs` entries so the legacy output is preserved).
pub fn list_source_files(repo: &Path, sha: &Sha) -> Result<Vec<PathBuf>> {
    let tree = git_tree_at(repo, sha)?;
    Ok(tree
        .into_iter()
        .filter(|p| Language::for_path(p).is_some())
        .collect())
}
```

Update every call site (search `extension().and_then(|e| e.to_str()) == Some("rs")` in `replay/mod.rs`) — replace with `Language::for_path(p).is_some()`. The `.md` filter on line 617 (doc-claim files) is separate; leave it alone for now.

- [ ] **Step 5: Run the characterization test + full suite**

```bash
cargo test -p provbench-labeler --test replay
cargo test -p provbench-labeler --release
```

Expected: PASS. If any existing test fails, the new walker is reordering Rust-only trees — fix before committing.

- [ ] **Step 6: Commit**

(Per the in-execution pacing decision recorded in commit `d4a38db`'s follow-up, the intermediate ripgrep canary check between Task 3 and Task 12 is dropped. The final Task 18 canary catches any cumulative Rust-path drift, which is sufficient for a no-op refactor of this scale. If Task 18 fails, `git bisect` between Tasks 3-18 is cheap.)

```bash
git add benchmarks/provbench/labeler/src/replay/mod.rs \
        benchmarks/provbench/labeler/tests/replay.rs
git commit -m "refactor(provbench-labeler): dispatch source-file walker via Language enum"
```

---

## Task 4: Add `tree-sitter-python` dependency

**Files:**
- Modify: `benchmarks/provbench/labeler/Cargo.toml`
- Modify: `benchmarks/provbench/labeler/Cargo.lock` (auto)

- [ ] **Step 1: Add the dependency**

Edit `benchmarks/provbench/labeler/Cargo.toml`. After `tree-sitter-rust = "0.24"`:

```toml
tree-sitter-python = "0.25"
```

- [ ] **Step 2: Verify it resolves to the SPEC-pinned grammar version**

```bash
cargo update -p tree-sitter-python --dry-run 2>&1
cargo metadata --format-version 1 -p provbench-labeler --filter-platform x86_64-unknown-linux-gnu \
  | jq -r '.packages[] | select(.name=="tree-sitter-python") | .version'
```

Expected: `0.25.0` exactly (or a 0.25.x patch — confirm patch versions are forward-compatible with the pinned grammar hash; if `0.25.x > 0.25.0`, document the delta in the labeler README and surface in Plan B's §11 row).

- [ ] **Step 3: Build to confirm the crate links**

```bash
cd benchmarks/provbench/labeler && cargo build --release && cd -
```

Expected: builds clean (the dep is present but unused — clippy `unused_crate_dependencies` is allowed by default).

- [ ] **Step 4: Commit**

```bash
git add benchmarks/provbench/labeler/Cargo.toml benchmarks/provbench/labeler/Cargo.lock
git commit -m "deps(provbench-labeler): add tree-sitter-python 0.25 (SPEC §13.1 pin)"
```

---

## Task 5: Implement `ast/python.rs` parser wrapper

**Files:**
- Create: `benchmarks/provbench/labeler/src/ast/python.rs`
- Modify: `benchmarks/provbench/labeler/src/ast/mod.rs` (add `pub mod python;` + re-export)
- Test: `benchmarks/provbench/labeler/tests/python_ast.rs`
- Create fixture: `benchmarks/provbench/labeler/tests/data/python/repo/src/example.py`

- [ ] **Step 1: Create the fixture file**

```python
# benchmarks/provbench/labeler/tests/data/python/repo/src/example.py
"""Example module for Python AST tests."""

CONSTANT_X = 42

class Greeter:
    """A simple greeter."""

    greeting: str = "hello"

    def greet(self, name: str) -> str:
        """Return a greeting for *name*."""
        return f"{self.greeting}, {name}!"

async def async_op(x: int) -> int:
    return x + 1

def _private(): ...
```

- [ ] **Step 2: Write the failing tests**

```rust
// benchmarks/provbench/labeler/tests/python_ast.rs
use provbench_labeler::ast::python::PythonAst;

const SRC: &str = include_str!("data/python/repo/src/example.py");

#[test]
fn parse_succeeds() {
    PythonAst::parse(SRC.as_bytes()).unwrap();
}

#[test]
fn function_signature_spans_lists_all_defs() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let mut sigs: Vec<_> = ast.function_signature_spans().collect();
    sigs.sort_by(|a, b| a.0.cmp(&b.0));
    let names: Vec<&str> = sigs.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["_private", "async_op", "greet"]);
}

#[test]
fn class_spans_lists_classes() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let classes: Vec<_> = ast.class_spans().collect();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].0, "Greeter");
}

#[test]
fn module_constant_spans_lists_uppercase_bindings() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let consts: Vec<_> = ast.module_constant_spans().collect();
    let names: Vec<&str> = consts.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["CONSTANT_X"]);
}

#[test]
fn signature_span_stops_before_body() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let (name, span) = ast
        .function_signature_spans()
        .find(|(n, _)| n == "greet")
        .unwrap();
    let signature_text = std::str::from_utf8(&SRC.as_bytes()[span.byte_range.clone()]).unwrap();
    assert!(signature_text.starts_with("def greet"));
    assert!(signature_text.ends_with("-> str:") || signature_text.ends_with("-> str"));
    assert!(!signature_text.contains("return"));
    assert_eq!(name, "greet");
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cargo test -p provbench-labeler --test python_ast
```

Expected: FAIL — `provbench_labeler::ast::python` does not exist.

- [ ] **Step 4: Implement the wrapper**

```rust
// benchmarks/provbench/labeler/src/ast/python.rs
//! Tree-sitter Python parser wrapper. Mirrors `RustAst` in module shape:
//! `parse`, `source`, `root`, plus per-fact-kind span iterators.
//! Span definitions:
//!  - function signature: from `def`/`async def` keyword through the `:`
//!    that opens the body (exclusive of the body block).
//!  - class: from the `class` keyword through the trailing `:` of the header.
//!  - module constant: assignment statements at module scope whose LHS is
//!    a single Name in SCREAMING_SNAKE_CASE (`[A-Z][A-Z0-9_]*`).

use anyhow::{Context, Result};
use tree_sitter::{Node, Parser, Tree};

use super::spans::Span;

pub struct PythonAst {
    src: Vec<u8>,
    tree: Tree,
}

impl PythonAst {
    pub fn parse(src: &[u8]) -> Result<Self> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .context("set python language")?;
        let tree = parser
            .parse(src, None)
            .ok_or_else(|| anyhow::anyhow!("tree-sitter returned no tree"))?;
        Ok(Self {
            src: src.to_vec(),
            tree,
        })
    }

    pub fn source(&self) -> &[u8] {
        &self.src
    }

    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }

    pub fn function_signature_spans(&self) -> impl Iterator<Item = (String, Span)> + '_ {
        crate::facts::python::function_signature::iter(self)
    }

    pub fn class_spans(&self) -> impl Iterator<Item = (String, Span)> + '_ {
        crate::facts::python::field::iter_classes(self)
    }

    pub fn module_constant_spans(&self) -> impl Iterator<Item = (String, Span)> + '_ {
        crate::facts::python::symbol_existence::iter_module_constants(self)
    }
}
```

Re-export from `ast/mod.rs`:

```rust
// benchmarks/provbench/labeler/src/ast/mod.rs (add)
pub mod python;
```

The Rust-only comment block in `ast/mod.rs:1-3` can be updated to drop the "Python support will be added in a sibling module" sentence since it's now true. Keep the rest of the file unchanged.

- [ ] **Step 5: Implement the `facts::python::*::iter` stubs**

At this point the AST module compiles but the `facts::python::*::iter` functions don't exist. Implement minimal versions that just walk the tree-sitter tree — full extractors come in Tasks 6-10. Stub bodies that compile and pass the AST tests:

```rust
// benchmarks/provbench/labeler/src/facts/python/mod.rs
pub mod function_signature;
pub mod field;
pub mod symbol_existence;
pub mod doc_claim;
pub mod test_assertion;
```

(Module dispatch — empty for now; the `iter*` fns below give the AST tests something to call.)

Walkers use `tree_sitter::QueryCursor` over the Python grammar:
- `function_signature::iter` matches `(function_definition name: (identifier) @name)` and returns the span from the `function_definition` start through the `:` token preceding the body (use `body: (block) ...`, then the `:` is the previous sibling).
- `field::iter_classes` matches `(class_definition name: (identifier) @name)` and spans the same way.
- `symbol_existence::iter_module_constants` matches module-scope `(expression_statement (assignment left: (identifier) @name))` where `@name` text matches `^[A-Z][A-Z0-9_]*$`.

Each `iter*` returns `impl Iterator<Item = (String, Span)>`. Use `unicode_xid` only if a name validation routine needs it — for SPEC §9.1 spot-check the bytes-exact form from the source is fine.

- [ ] **Step 6: Run AST tests to verify they pass**

```bash
cargo test -p provbench-labeler --test python_ast --release
```

Expected: all four tests PASS.

- [ ] **Step 7: Run full suite — Rust path must remain green**

```bash
cargo test -p provbench-labeler --release
```

Expected: PASS. Re-run the canary SHA check from Task 3 Step 6 (`run` on ripgrep + strip `labeler_git_sha` + sha256sum) — SHA must still match the pre-baseline.

- [ ] **Step 8: Commit**

```bash
git add benchmarks/provbench/labeler/src/ast/ \
        benchmarks/provbench/labeler/src/facts/python/ \
        benchmarks/provbench/labeler/tests/data/python/ \
        benchmarks/provbench/labeler/tests/python_ast.rs
git commit -m "feat(provbench-labeler): add Python AST wrapper + span iterators"
```

---

## Task 6: Python `function_signature` fact extractor

**Files:**
- Modify: `benchmarks/provbench/labeler/src/facts/python/function_signature.rs`
- Modify: `benchmarks/provbench/labeler/src/facts/mod.rs` (dispatch by Language)
- Test: `benchmarks/provbench/labeler/tests/python_facts.rs`

- [ ] **Step 1: Write the failing test**

```rust
// benchmarks/provbench/labeler/tests/python_facts.rs  (new file)
use provbench_labeler::facts::{Fact, FactKind, extract_for_path};
use std::path::PathBuf;

const SRC: &str = include_str!("data/python/repo/src/example.py");

#[test]
fn function_signature_emits_one_fact_per_def() {
    let facts = extract_for_path(&PathBuf::from("src/example.py"), SRC.as_bytes()).unwrap();
    let mut sigs: Vec<_> = facts
        .iter()
        .filter(|f| f.kind == FactKind::FunctionSignature)
        .cloned()
        .collect();
    sigs.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    let names: Vec<&str> = sigs.iter().map(|f| f.qualified_name.as_str()).collect();
    assert_eq!(names, vec!["src.example.Greeter.greet", "src.example._private", "src.example.async_op"]);
}

#[test]
fn function_signature_content_hash_is_signature_only() {
    let facts = extract_for_path(&PathBuf::from("src/example.py"), SRC.as_bytes()).unwrap();
    let greet = facts.iter().find(|f| f.qualified_name == "src.example.Greeter.greet").unwrap();
    // Mutating the body must NOT change content_hash; mutating the signature MUST.
    let mutated_body = SRC.replace(
        "return f\"{self.greeting}, {name}!\"",
        "return self.greeting + ', ' + name",
    );
    let after_body = extract_for_path(&PathBuf::from("src/example.py"), mutated_body.as_bytes()).unwrap();
    let greet_after = after_body.iter().find(|f| f.qualified_name == "src.example.Greeter.greet").unwrap();
    assert_eq!(greet.content_hash, greet_after.content_hash, "body change leaked into signature hash");

    let mutated_sig = SRC.replace("def greet(self, name: str) -> str:", "def greet(self, name: str) -> bytes:");
    let after_sig = extract_for_path(&PathBuf::from("src/example.py"), mutated_sig.as_bytes()).unwrap();
    let greet_sig_changed = after_sig.iter().find(|f| f.qualified_name == "src.example.Greeter.greet").unwrap();
    assert_ne!(greet.content_hash, greet_sig_changed.content_hash, "signature change did not affect content_hash");
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p provbench-labeler --test python_facts
```

Expected: FAIL — `extract_for_path` doesn't yet dispatch by language to a Python extractor.

- [ ] **Step 3: Implement the extractor**

```rust
// benchmarks/provbench/labeler/src/facts/python/function_signature.rs
use crate::ast::python::PythonAst;
use crate::ast::spans::Span;
use anyhow::Result;
use tree_sitter::{Node, QueryCursor};

pub fn iter(ast: &PythonAst) -> impl Iterator<Item = (String, Span)> + '_ {
    let mut out = Vec::new();
    walk(ast.root(), ast.source(), &mut Vec::new(), &mut out);
    out.into_iter()
}

fn walk<'a>(node: Node<'a>, src: &[u8], path: &mut Vec<String>, out: &mut Vec<(String, Span)>) {
    match node.kind() {
        "module" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, src, path, out);
            }
        }
        "class_definition" => {
            let name = node.child_by_field_name("name").and_then(|n| n.utf8_text(src).ok()).unwrap_or("?").to_string();
            path.push(name);
            if let Some(body) = node.child_by_field_name("body") {
                let mut c = body.walk();
                for child in body.children(&mut c) {
                    walk(child, src, path, out);
                }
            }
            path.pop();
        }
        "function_definition" => {
            let name = node.child_by_field_name("name").and_then(|n| n.utf8_text(src).ok()).unwrap_or("?").to_string();
            let span = signature_span(node);
            let qualified = if path.is_empty() {
                name
            } else {
                format!("{}.{}", path.join("."), name)
            };
            out.push((qualified, span));
            // Do not descend into bodies; nested defs are addressed by symbol_existence, not signatures.
        }
        _ => {}
    }
}

fn signature_span(fn_node: Node<'_>) -> Span {
    let body = fn_node.child_by_field_name("body").expect("function_definition has body");
    let body_start = body.start_byte();
    Span {
        byte_range: fn_node.start_byte()..body_start,
        line_start: (fn_node.start_position().row + 1) as u32,
        line_end: (body.start_position().row + 1) as u32,
    }
}
```

- [ ] **Step 4: Wire into `facts::extract_for_path`**

Modify `benchmarks/provbench/labeler/src/facts/mod.rs` so `extract_for_path` dispatches by `Language::for_path`:

```rust
pub fn extract_for_path(path: &Path, src: &[u8]) -> Result<Vec<Fact>> {
    match crate::lang::Language::for_path(path) {
        Some(Language::Rust) => extract_rust(path, src),
        Some(Language::Python) => extract_python(path, src),
        None => Ok(Vec::new()),
    }
}
```

Implement `extract_python(path, src)` to walk `PythonAst`, emit one `Fact::FunctionSignature` per `function_signature_spans()` item, and compute `content_hash = sha256(signature_text)`. Module path (`src.example` in the test) is computed by stripping leading `src/` and the `.py` suffix and replacing `/` with `.` — match SPEC §4 conventions if they exist, otherwise document the rule in the labeler README.

- [ ] **Step 5: Run tests + commit**

```bash
cargo test -p provbench-labeler --test python_facts
cargo test -p provbench-labeler --release
git add benchmarks/provbench/labeler/src/facts/ \
        benchmarks/provbench/labeler/tests/python_facts.rs
git commit -m "feat(provbench-labeler): emit Python FunctionSignature facts"
```

Expected: PASS. Rust canary SHA still byte-stable (re-verify per Task 3 Step 6).

---

## Task 7: Python `field` fact extractor

**Files:**
- Modify: `benchmarks/provbench/labeler/src/facts/python/field.rs`
- Test: `benchmarks/provbench/labeler/tests/python_facts.rs` (append)

**Scope:** Python `Field` fact = class attribute (annotated assignment in class body OR plain assignment in class body OR dataclass field). Excludes function-local variables and instance attributes assigned in `__init__` (these are addressed by `symbol_existence`, not `field`).

- [ ] **Step 1: Append failing tests**

```rust
#[test]
fn field_emits_one_per_class_attribute() {
    let facts = extract_for_path(&PathBuf::from("src/example.py"), SRC.as_bytes()).unwrap();
    let fields: Vec<&Fact> = facts.iter().filter(|f| f.kind == FactKind::Field).collect();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].qualified_name, "src.example.Greeter.greeting");
}

#[test]
fn field_content_hash_covers_type_annotation() {
    let mutated = SRC.replace("greeting: str = \"hello\"", "greeting: bytes = b\"hello\"");
    let before = extract_for_path(&PathBuf::from("src/example.py"), SRC.as_bytes()).unwrap();
    let after  = extract_for_path(&PathBuf::from("src/example.py"), mutated.as_bytes()).unwrap();
    let bf = before.iter().find(|f| f.qualified_name == "src.example.Greeter.greeting").unwrap();
    let af = after.iter().find(|f| f.qualified_name == "src.example.Greeter.greeting").unwrap();
    assert_ne!(bf.content_hash, af.content_hash);
}
```

- [ ] **Step 2: Implement extractor (skeleton — full body in plan execution)**

Tree-sitter query: walk `class_definition > body > expression_statement` looking for `assignment` whose `left` is either `(identifier)` or `(attribute object: (identifier) "self" attribute: ...)` — but for class-level fields only the bare `(identifier)` form counts; `self.X` inside `__init__` is excluded.

Also handle `(class_definition body: (block (expression_statement (assignment left: (identifier)? type: (type ...)?))))` — Python typed class attributes (the test fixture uses `greeting: str = "hello"`).

The content hash covers the full `assignment` node text (LHS + annotation + RHS) so that the test_assertion-style test in Step 1 passes.

- [ ] **Step 3: Wire into `extract_python` + commit**

```bash
cargo test -p provbench-labeler --test python_facts
git add benchmarks/provbench/labeler/src/facts/python/field.rs \
        benchmarks/provbench/labeler/src/facts/mod.rs \
        benchmarks/provbench/labeler/tests/python_facts.rs
git commit -m "feat(provbench-labeler): emit Python Field facts (class attributes)"
```

Expected: PASS. Rust canary still byte-stable.

---

## Task 8: Python `symbol_existence` fact extractor

**Files:**
- Modify: `benchmarks/provbench/labeler/src/facts/python/symbol_existence.rs`
- Test: `benchmarks/provbench/labeler/tests/python_facts.rs` (append)

**Scope:** Python `SymbolExistence` fact = module-level binding (function, class, top-level assignment). Mirrors the Rust extractor in `facts/symbol_existence.rs` — a `Symbol` fact's existence is what R1 (stale_source_deleted) tests against. Per-fact content hash is computed over the *header* (def line / class line / assignment target), not the body — body changes are R4's job, not R1's.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn symbol_existence_lists_all_module_bindings() {
    let facts = extract_for_path(&PathBuf::from("src/example.py"), SRC.as_bytes()).unwrap();
    let mut symbols: Vec<&str> = facts
        .iter()
        .filter(|f| f.kind == FactKind::SymbolExistence)
        .map(|f| f.qualified_name.as_str())
        .collect();
    symbols.sort();
    assert_eq!(
        symbols,
        vec![
            "src.example.CONSTANT_X",
            "src.example.Greeter",
            "src.example._private",
            "src.example.async_op",
        ]
    );
}
```

- [ ] **Step 2: Implement extractor + wire + commit**

Tree-sitter queries:
- `(module (function_definition name: (identifier) @name))` → module-level defs
- `(module (class_definition name: (identifier) @name))` → module-level classes
- `(module (expression_statement (assignment left: (identifier) @name)))` → module-level assignments (regardless of SCREAMING_SNAKE_CASE — Field already covers class-scoped only).

```bash
cargo test -p provbench-labeler --test python_facts
git add benchmarks/provbench/labeler/src/facts/python/symbol_existence.rs \
        benchmarks/provbench/labeler/tests/python_facts.rs
git commit -m "feat(provbench-labeler): emit Python SymbolExistence facts (module bindings)"
```

---

## Task 9: Python `doc_claim` fact extractor

**Files:**
- Modify: `benchmarks/provbench/labeler/src/facts/python/doc_claim.rs`
- Test: `benchmarks/provbench/labeler/tests/python_facts.rs` (append)
- Fixture: add `benchmarks/provbench/labeler/tests/data/python/repo/README.md`

**Scope:** Same SPEC §4 fact kind as Rust `doc_claim` — a `(file.md|module docstring|class docstring|function docstring)` → `qualified_symbol_name` pair. The README extractor uses `pulldown-cmark` to find ```` ```python ``` ```` fenced blocks or inline `` `module.Class.method` `` references; the docstring extractor finds bare `` `name` `` references inside docstrings.

A doc-claim's `qualified_name` is the *symbol it references*; its `content_hash` covers the surrounding text (the markdown paragraph or docstring line). R5 (stale_doc_drift) compares this hash across commits.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn doc_claim_extracts_module_docstring_references() {
    let src = r#"
"""See `package.utils.helper` for details."""
def f(): pass
"#;
    let facts = extract_for_path(&PathBuf::from("src/example.py"), src.as_bytes()).unwrap();
    let claims: Vec<&Fact> = facts.iter().filter(|f| f.kind == FactKind::DocClaim).collect();
    assert!(claims.iter().any(|c| c.qualified_name == "package.utils.helper"));
}

#[test]
fn doc_claim_extracts_readme_python_refs() {
    let md = "See `flask.app.Flask.run` for the entry point.";
    let facts = extract_for_path(&PathBuf::from("README.md"), md.as_bytes()).unwrap();
    let claims: Vec<&Fact> = facts.iter().filter(|f| f.kind == FactKind::DocClaim).collect();
    assert!(claims.iter().any(|c| c.qualified_name == "flask.app.Flask.run"));
}
```

- [ ] **Step 2: Implement + wire + commit**

`README.md` is a non-Rust, non-Python path: the existing Rust path already routes `.md` to `doc_claim` (replay/mod.rs line 617). The Python doc-claim extractor adds a Python-flavored namespace pattern (`pkg.mod.sym`). Detect Python-style references by:
1. Token shape: dotted identifier matching `[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*`
2. Not a Rust path (`::`-separated)
3. Either inside a ```` ```python ```` fenced block or in inline-code (`` ` `` ... `` ` ``)

```bash
cargo test -p provbench-labeler --test python_facts
git add benchmarks/provbench/labeler/src/facts/python/doc_claim.rs \
        benchmarks/provbench/labeler/tests/data/python/repo/README.md \
        benchmarks/provbench/labeler/tests/python_facts.rs
git commit -m "feat(provbench-labeler): emit Python DocClaim facts (docstrings + README refs)"
```

---

## Task 10: Python `test_assertion` fact extractor

**Files:**
- Modify: `benchmarks/provbench/labeler/src/facts/python/test_assertion.rs`
- Fixture: `benchmarks/provbench/labeler/tests/data/python/repo/tests/test_example.py`
- Test: `benchmarks/provbench/labeler/tests/python_facts.rs` (append)

**Scope:** Python `TestAssertion` fact = a test function (function whose name starts with `test_` or in a class whose name starts with `Test`) that contains a `pytest.raises`, `pytest.warns`, `assert X == Y`, `assertEqual`, `assertRaises`, `unittest.TestCase.assert*` call. `content_hash` covers the assertion expression body (the right-hand side of `assert` or the args of `assertX`).

- [ ] **Step 1: Add fixture**

```python
# benchmarks/provbench/labeler/tests/data/python/repo/tests/test_example.py
import pytest
from src.example import Greeter

def test_greet_returns_hello():
    assert Greeter().greet("world") == "hello, world!"

class TestGreeter:
    def test_default_greeting(self):
        assert Greeter().greeting == "hello"
```

- [ ] **Step 2: Failing tests + implementation + commit**

Tree-sitter queries: `(function_definition name: (identifier) @name (#match? @name "^test_"))` and `(class_definition name: (identifier) @c (#match? @c "^Test") body: (block (function_definition name: (identifier) @name)))`. Inside each test fn, extract `assert_statement` and `call expression (attribute object: (identifier) (#eq? @ "self") attribute: (identifier) @method (#match? @method "^assert"))`.

```bash
cargo test -p provbench-labeler --test python_facts
git add benchmarks/provbench/labeler/src/facts/python/test_assertion.rs \
        benchmarks/provbench/labeler/tests/data/python/repo/tests/test_example.py \
        benchmarks/provbench/labeler/tests/python_facts.rs
git commit -m "feat(provbench-labeler): emit Python TestAssertion facts (assert + unittest)"
```

---

## Task 11: Python lexical scope walker + import graph

**Files:**
- Create: `benchmarks/provbench/labeler/src/resolve/python.rs` (may split into `python/mod.rs` + `python/import_graph.rs` + `python/scope.rs` if it grows past 400 lines)
- Modify: `benchmarks/provbench/labeler/src/resolve/mod.rs` (add `pub mod python;`)
- Test: `benchmarks/provbench/labeler/tests/python_resolve.rs`

**Scope:** Implement the `SymbolResolver` trait (`resolve/mod.rs:16`) for Python. Required behaviors:
1. Given a qualified name like `src.example.Greeter.greet`, return `ResolvedLocation { file: src/example.py, line: <def line> }`.
2. Given a name like `flask.app.Flask` referenced from `tests/test_views.py`, walk that file's imports, build the import graph, and resolve to the canonical defining file (`src/flask/app.py`).
3. **Out of scope** (documented limitations): star-imports beyond top-level re-exports, dynamic `__all__` mutation, conditional imports under `if TYPE_CHECKING`, namespace packages.

- [ ] **Step 1: Failing resolver tests**

```rust
// benchmarks/provbench/labeler/tests/python_resolve.rs
use provbench_labeler::resolve::python::PythonResolver;
use provbench_labeler::resolve::SymbolResolver;

#[test]
fn resolves_module_function() {
    let mut r = PythonResolver::index(common::python_fixture_repo()).unwrap();
    let loc = r.resolve("src.example.async_op").unwrap().unwrap();
    assert!(loc.file.ends_with("src/example.py"));
}

#[test]
fn resolves_class_method() {
    let mut r = PythonResolver::index(common::python_fixture_repo()).unwrap();
    let loc = r.resolve("src.example.Greeter.greet").unwrap().unwrap();
    assert!(loc.file.ends_with("src/example.py"));
}

#[test]
fn resolves_through_import() {
    // tests/test_example.py imports `from src.example import Greeter`.
    let mut r = PythonResolver::index(common::python_fixture_repo()).unwrap();
    let loc = r.resolve("tests.test_example.Greeter").unwrap().unwrap();
    assert!(loc.file.ends_with("src/example.py"));
}

#[test]
fn unresolved_returns_none() {
    let mut r = PythonResolver::index(common::python_fixture_repo()).unwrap();
    assert!(r.resolve("src.example.does_not_exist").unwrap().is_none());
}
```

(Add a `python_fixture_repo()` helper to `tests/common/mod.rs` that returns a `PathBuf` to a tempfile-cloned copy of `tests/data/python/repo/`.)

- [ ] **Step 2: Implementation outline**

```rust
// benchmarks/provbench/labeler/src/resolve/python.rs
//! Tree-sitter scope chains + lexical import graph.
//!
//! Indexing pass (per repo @ HEAD):
//!   1. Walk every `.py` file under `repo/`.
//!   2. For each file `f`:
//!       a. Parse with PythonAst.
//!       b. Record all module-level bindings (functions, classes, assignments,
//!          imports) keyed by `(module_path, name)`.
//!       c. Record class-body bindings keyed by `(module_path, class, name)`.
//!       d. Record `import ...` / `from ... import ...` statements as edges
//!          in `ImportGraph`.
//!   3. After all files are walked, resolve edges to canonical defining files
//!      (file path of the bound symbol). Star-imports re-export only what is
//!      in `__all__` if present; otherwise we conservatively skip them.
//!
//! Resolution pass (per qualified_name query):
//!   1. Split on `.`.
//!   2. Find the longest module-path prefix that exists in the index.
//!   3. The remaining segments must be class/function/attribute names within
//!      that module; descend through the per-symbol map.
//!   4. If any segment is an import, follow the edge once (one hop) and retry.
//!   5. Return ResolvedLocation pointing at the def line, or None.

use crate::ast::python::PythonAst;
use crate::resolve::{ResolvedLocation, SymbolResolver};
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct PythonResolver {
    repo_root: PathBuf,
    /// (module_path, top_level_name) -> def location
    module_bindings: BTreeMap<(String, String), ResolvedLocation>,
    /// (module_path, class_name, member_name) -> def location
    class_bindings: BTreeMap<(String, String, String), ResolvedLocation>,
    /// (importing_module, local_alias) -> (target_module, target_name)
    import_graph: BTreeMap<(String, String), (String, String)>,
}

impl PythonResolver {
    pub fn index(repo_root: impl Into<PathBuf>) -> Result<Self> { todo!() }

    fn module_path_for(&self, file: &Path) -> Option<String> {
        // Strip `repo_root` prefix and `.py` suffix; replace `/` with `.`.
        // `__init__.py` collapses to its directory's module path.
        todo!()
    }
}

impl SymbolResolver for PythonResolver {
    fn resolve(&mut self, qualified_name: &str) -> Result<Option<ResolvedLocation>> {
        // 1. Try longest-prefix module match.
        // 2. Walk remaining segments through module_bindings / class_bindings.
        // 3. On a miss, check import_graph for one-hop redirect.
        todo!()
    }
}
```

Implement the `todo!()` bodies. Determinism: every map is `BTreeMap`, not `HashMap`. Iteration over a directory must use `walkdir` with a sorted file-name order (or `read_dir` + `collect::<Vec<_>>` + `.sort()`).

- [ ] **Step 3: Run tests + commit**

```bash
cargo test -p provbench-labeler --test python_resolve
git add benchmarks/provbench/labeler/src/resolve/ \
        benchmarks/provbench/labeler/tests/python_resolve.rs \
        benchmarks/provbench/labeler/tests/common/mod.rs
git commit -m "feat(provbench-labeler): add PythonResolver (tree-sitter scope + import graph)"
```

Expected: PASS.

---

## Task 12: Wire Python facts into replay corpus emission

**Files:**
- Modify: `benchmarks/provbench/labeler/src/replay/mod.rs`
- Modify: `benchmarks/provbench/labeler/src/replay/commit_index.rs`
- Test: `benchmarks/provbench/labeler/tests/replay.rs` (existing tests must remain green; add a Python-only test)

- [ ] **Step 1: Add the failing Python replay test**

```rust
// benchmarks/provbench/labeler/tests/replay.rs (append)
#[test]
fn replay_python_fixture_emits_facts() {
    let repo = common::python_fixture_repo_git();   // tempfile-backed gix repo with one commit
    let cfg = ReplayConfig::python_default();
    let out = replay(&repo, "HEAD", cfg).unwrap();
    let py_facts: Vec<_> = out.iter()
        .filter(|f| f.path.to_string_lossy().ends_with(".py"))
        .collect();
    assert!(!py_facts.is_empty(), "no Python facts emitted");
    // Determinism: replay twice and assert identical sequence.
    let out2 = replay(&repo, "HEAD", cfg).unwrap();
    assert_eq!(out, out2);
}
```

- [ ] **Step 2: Add Python branch in `replay/mod.rs`**

The current replay loop iterates `list_source_files` (Task 3's generic walker). For each path, dispatch by `Language::for_path`:
- `Language::Rust`: existing path (RustAst + facts::rust).
- `Language::Python`: new path (PythonAst + facts::python + PythonResolver for cross-file symbol resolution in R4 line-presence and R7 rename-candidate proxies).

`commit_index.rs` builds a commit-local symbol index — currently Rust-only. Generalize so it stores symbols keyed by `(language, module_path, symbol_name)` and the per-rule queries can fan out per-language. R3/R4/R5/R7 rules are language-agnostic at the fact-row level (they consume `Fact` rows, not AST nodes), so phase1 needs zero changes. Only the labeler's index needs to know about Python.

- [ ] **Step 3: Run replay test + Rust replay tests**

```bash
cargo test -p provbench-labeler --test replay --release
```

Expected: PASS. The pre-existing Rust replay tests must remain byte-identical; the new Python replay test must pass.

- [ ] **Step 4: Commit**

(Intermediate ripgrep canary at this point is dropped per the same pacing decision as Task 3 Step 6. Task 18 catches cumulative drift. If Task 18 fails, bisect.)

```bash
git add benchmarks/provbench/labeler/src/replay/ \
        benchmarks/provbench/labeler/tests/replay.rs
git commit -m "feat(provbench-labeler): replay dispatches by Language for Python corpora"
```

---

## Task 13: Python diff handling — whitespace + rename detection

**Files:**
- Modify: `benchmarks/provbench/labeler/src/diff/mod.rs`
- Modify: `benchmarks/provbench/labeler/src/replay/match_post.rs`
- Test: `benchmarks/provbench/labeler/tests/diff.rs` (append Python-specific cases)

**Scope:** The whitespace/comment-only detector currently handles Rust (`//` line comments, `/* */` blocks). For Python, replace with `#` line comments and triple-quoted strings `"""..."""` and `'''...'''`. The detector returns `IsTrivial::Yes` for diffs whose only changes are within comments/strings; this matters for R2 (`whitespace_only_change` rule).

The rename detector in `match_post.rs` is a Myers-diff-based file-stem matcher — already language-agnostic. Audit it for any hard-coded `.rs` extension references and switch them to use `Language::for_path` if found.

- [ ] **Step 1: Add failing test**

```rust
// benchmarks/provbench/labeler/tests/diff.rs (append)
#[test]
fn python_whitespace_only_detected() {
    let before = "def f():\n    return 1\n";
    let after  = "def f():\n    return 1\n\n";  // trailing newline only
    assert!(is_trivial_diff(before, after, Language::Python));
}

#[test]
fn python_comment_only_detected() {
    let before = "def f():\n    return 1  # ok\n";
    let after  = "def f():\n    return 1  # OK!\n";
    assert!(is_trivial_diff(before, after, Language::Python));
}

#[test]
fn python_body_change_not_trivial() {
    let before = "def f():\n    return 1\n";
    let after  = "def f():\n    return 2\n";
    assert!(!is_trivial_diff(before, after, Language::Python));
}
```

- [ ] **Step 2: Implement + commit**

```bash
cargo test -p provbench-labeler --test diff
git add benchmarks/provbench/labeler/src/diff/ \
        benchmarks/provbench/labeler/src/replay/match_post.rs \
        benchmarks/provbench/labeler/tests/diff.rs
git commit -m "feat(provbench-labeler): Python-aware whitespace/comment diff detection"
```

Expected: PASS.

---

## Task 14: Spotcheck CLI — accept `--lang=python`

**Files:**
- Modify: `benchmarks/provbench/labeler/src/spotcheck.rs`
- Modify: `benchmarks/provbench/labeler/src/main.rs`
- Test: `benchmarks/provbench/labeler/tests/cli_spotcheck_python.rs`

**Scope:** Existing `provbench-labeler spotcheck` emits a 200-row CSV with labels hidden, used for human review per SPEC §9.1. Add `--lang={rust,python,both}` (default `both`); when `python` or `both`, the stratified sampler treats Python facts as part of the stratum population. CSV column structure unchanged.

- [ ] **Step 1: Failing CLI test**

```rust
// benchmarks/provbench/labeler/tests/cli_spotcheck_python.rs
use assert_cmd::Command;

#[test]
fn spotcheck_python_lang_filter() {
    let repo = common::python_fixture_repo();
    let out  = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("provbench-labeler").unwrap()
        .args(["spotcheck", "--repo", repo.to_str().unwrap(),
               "--corpus", "/dev/stdin", "--out", out.path().to_str().unwrap(),
               "--lang", "python", "--n", "10", "--seed", "42"])
        .write_stdin(common::python_corpus_jsonl(&repo))
        .assert()
        .success();
    let csv = std::fs::read_to_string(out.path()).unwrap();
    assert!(csv.lines().count() >= 11);              // header + 10 rows
    assert!(csv.lines().skip(1).all(|l| l.contains(".py")));
}
```

- [ ] **Step 2: Implement + wire + commit**

Add a `Lang` enum to clap `#[derive(ValueEnum)]`. Stratification logic in `spotcheck.rs` filters the corpus by `Language::for_path(path)` before sampling.

```bash
cargo test -p provbench-labeler --test cli_spotcheck_python
git add benchmarks/provbench/labeler/src/spotcheck.rs \
        benchmarks/provbench/labeler/src/main.rs \
        benchmarks/provbench/labeler/tests/cli_spotcheck_python.rs
git commit -m "feat(provbench-labeler): spotcheck --lang={rust,python,both}"
```

---

## Task 15: Python determinism — fixture-level

**Files:**
- Create: `benchmarks/provbench/labeler/tests/determinism_python.rs`

- [ ] **Step 1: Failing test**

```rust
// benchmarks/provbench/labeler/tests/determinism_python.rs
use sha2::{Digest, Sha256};

#[test]
fn python_fixture_corpus_byte_identical_across_runs() {
    let repo = common::python_fixture_repo_git();
    let a = run_emit_corpus(&repo);
    let b = run_emit_corpus(&repo);
    assert_eq!(Sha256::digest(&a), Sha256::digest(&b));
}

#[test]
fn python_fixture_facts_byte_identical_across_runs() {
    let repo = common::python_fixture_repo_git();
    let a = run_emit_facts(&repo);
    let b = run_emit_facts(&repo);
    assert_eq!(Sha256::digest(&a), Sha256::digest(&b));
}

#[test]
fn python_fixture_diffs_byte_identical_across_runs() {
    let repo = common::python_fixture_repo_git();
    let a = run_emit_diffs(&repo);
    let b = run_emit_diffs(&repo);
    assert_eq!(Sha256::digest(&a), Sha256::digest(&b));
}
```

- [ ] **Step 2: Run + commit**

```bash
cargo test -p provbench-labeler --test determinism_python --release
git add benchmarks/provbench/labeler/tests/determinism_python.rs
git commit -m "test(provbench-labeler): fixture-level Python determinism (corpus/facts/diffs)"
```

Expected: PASS. If diffs vary, the Python path has nondeterminism (likely HashMap iteration somewhere — find via `git grep "HashMap" benchmarks/provbench/labeler/src/`).

---

## Task 16: Clone flask + verify gitignore

**Files:**
- Modify: `.gitignore` (only if `benchmarks/provbench/work/flask` isn't covered by the existing `work/` rule)
- External: `benchmarks/provbench/work/flask/` (gitignored)

- [ ] **Step 1: Confirm work/ is already gitignored**

```bash
git check-ignore -v benchmarks/provbench/work/flask 2>&1
```

Expected output names an existing `.gitignore` rule. If it doesn't, append `benchmarks/provbench/work/` to top-level `.gitignore` and commit before continuing.

- [ ] **Step 2: Clone flask at the pinned SHA**

```bash
git clone https://github.com/pallets/flask benchmarks/provbench/work/flask
git -C benchmarks/provbench/work/flask fetch origin
git -C benchmarks/provbench/work/flask checkout 2f0c62f5e6e290843f03c1fa70817c7a3c7fd661
git -C benchmarks/provbench/work/flask rev-parse HEAD
# expect: 2f0c62f5e6e290843f03c1fa70817c7a3c7fd661
```

- [ ] **Step 3: Verify the checkout is gitignored**

```bash
git status --short benchmarks/provbench/work/flask
# expect: empty
git check-ignore benchmarks/provbench/work/flask
# expect: prints the path (ignored)
```

---

## Task 17: Python determinism — full flask corpus (ignored test)

**Files:**
- Create: `benchmarks/provbench/labeler/tests/determinism_flask.rs`

- [ ] **Step 1: Failing test**

```rust
// benchmarks/provbench/labeler/tests/determinism_flask.rs
//! Full-flask two-run byte-identical determinism. Opt-in via
//! `cargo test -p provbench-labeler -- --ignored determinism_flask`.

use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[test]
#[ignore]
fn flask_corpus_byte_identical_across_runs() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../work/flask");
    assert!(repo.exists(), "missing work/flask checkout — see Task 16");
    let a = run_emit_corpus(&repo);
    let b = run_emit_corpus(&repo);
    let ha = Sha256::digest(&a);
    let hb = Sha256::digest(&b);
    if ha != hb {
        std::fs::write("/tmp/flask-corpus-a.jsonl", &a).unwrap();
        std::fs::write("/tmp/flask-corpus-b.jsonl", &b).unwrap();
        panic!("nondeterministic: see /tmp/flask-corpus-{{a,b}}.jsonl");
    }
}

#[test]
#[ignore]
fn flask_facts_byte_identical_across_runs() {
    /* analogous */
}

#[test]
#[ignore]
fn flask_diffs_byte_identical_across_runs() {
    /* analogous */
}
```

- [ ] **Step 2: Run the ignored test**

```bash
cargo test -p provbench-labeler --release -- --ignored determinism_flask
```

Expected: three tests PASS. If any fail, write the divergent artifacts to `/tmp/flask-*-{a,b}.jsonl` and run `diff` — typical culprits: HashMap iteration order, `gix` walking order, parallel rayon iteration without sort. Fix at the source, not by sorting after the fact.

- [ ] **Step 3: Commit**

```bash
git add benchmarks/provbench/labeler/tests/determinism_flask.rs
git commit -m "test(provbench-labeler): full-flask Python determinism (corpus/facts/diffs, #[ignore])"
```

---

## Task 18: Final Rust canary regression check

**Files:**
- Read-only: existing baseline at `docs/superpowers/plans/work/2026-05-15-python-labeler-canary-pre.txt`

- [ ] **Step 1: Rebuild + re-run canary**

```bash
cd benchmarks/provbench/labeler && cargo build --release && cd -
mkdir -p /tmp/canary-final
benchmarks/provbench/labeler/target/release/provbench-labeler run \
  --repo benchmarks/provbench/work/ripgrep \
  --t0   af6b6c543b224d348a8876f0c06245d9ea7929c5 \
  --out  /tmp/canary-final/corpus.jsonl
POST=$(jq -c 'del(.labeler_git_sha)' /tmp/canary-final/corpus.jsonl | sha256sum | cut -d' ' -f1)
PRE=$(cat docs/superpowers/plans/work/2026-05-15-python-labeler-canary-pre.txt)
test "$POST" = "$PRE" && echo "canary stable: $POST" || { echo "DRIFT: pre=$PRE post=$POST"; exit 1; }
```

Expected: `canary stable: <sha>`. This is the load-bearing assertion that no Plan A change leaked into the Rust path.

- [ ] **Step 2: Re-run the existing Rust spot-check + hardening suites unchanged**

```bash
cargo test -p provbench-labeler --release -- spotcheck replay_hardening
```

Expected: PASS.

- [ ] **Step 3: Format + lint**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both clean. Per the user's standing rule (memory `feedback_format_before_commit.md`), this is mandatory before every commit and especially before merging.

---

## Task 19: Python §9.1 spot-check on flask (200 samples)

**Files:**
- Create: `benchmarks/provbench/results/python-labeler-2026-05-15-spotcheck.csv` (committed)
- Create: `benchmarks/provbench/results/python-labeler-2026-05-15-spotcheck-findings.md` (committed)

- [ ] **Step 1: Emit the flask corpus**

```bash
SHA7=$(benchmarks/provbench/labeler/target/release/provbench-labeler stamp | cut -c1-7)
benchmarks/provbench/labeler/target/release/provbench-labeler run \
  --repo benchmarks/provbench/work/flask \
  --t0   2f0c62f5e6e290843f03c1fa70817c7a3c7fd661 \
  --out  benchmarks/provbench/corpus/flask-2f0c62f5-$SHA7.jsonl
```

(`stamp` returns the labeler's build-time git SHA; the short form goes in the filename.)

- [ ] **Step 2: Sample 200 Python facts with the spotcheck CLI**

```bash
benchmarks/provbench/labeler/target/release/provbench-labeler spotcheck \
  --repo benchmarks/provbench/work/flask \
  --corpus benchmarks/provbench/corpus/flask-2f0c62f5-$SHA7.jsonl \
  --out benchmarks/provbench/results/python-labeler-2026-05-15-spotcheck.csv \
  --lang python \
  --n 200 \
  --seed 13897750829054410479
```

(Seed `13897750829054410479` = `0xC0DEBABEDEADBEEF`; pass as decimal — clap's default `u64` parser does NOT accept hex. Memory `project_provbench_labeler_pin_quirks.md` flags this gotcha.)

- [ ] **Step 3: Human review**

Open the CSV. For each row, judge whether the machine label is correct against your reading of the source file at the recorded commit. Mark each as `correct` / `wrong`. Compute Wilson 95% lower bound.

- [ ] **Step 4: Write findings doc**

Sketch for `python-labeler-2026-05-15-spotcheck-findings.md`:

```markdown
# Python labeler spot-check — 2026-05-15

**Corpus:** `flask-2f0c62f5-<labeler-sha>.jsonl`
**Sample:** 200 facts, seed `13897750829054410479`, lang `python`
**Reviewer:** <name>
**Date:** 2026-05-15

| Outcome | Count |
|---|---|
| Correct | <n_correct> |
| Wrong | <n_wrong> |

**Wilson 95% lower bound:** <p_lb>

**Pass/Fail vs SPEC §9.1 (≥0.95):** PASS / FAIL

## Wrong-label triage
[Per-row notes on each "wrong" — fact kind, error mode, whether the extractor needs to be tightened or the kind dropped for Python.]

## Known limitations
- Star-import re-exports: handled only via __all__ when present.
- Dynamic dispatch / metaclass attribute generation: ignored.
- TYPE_CHECKING-conditional imports: ignored (would need a second-class scope).

## Decision
[If PASS] Python labeler accepted; pin SHA to <plan-A-merge-SHA> for use in Plan B (`2026-05-15-provbench-v1.2b-flask-heldout.md`).
[If FAIL] Triage above. Either tighten the worst extractor or drop the fact kind for Python; do NOT lower the §9.1 threshold.
```

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/results/python-labeler-2026-05-15-spotcheck.csv \
        benchmarks/provbench/results/python-labeler-2026-05-15-spotcheck-findings.md
git commit -m "data(provbench-labeler): Python labeler §9.1 spot-check on flask (n=200)"
```

---

## Task 20: Update labeler README

**Files:**
- Modify: `benchmarks/provbench/labeler/README.md`

- [ ] **Step 1: Add Python usage section**

Append a section after the existing Rust usage docs:

```markdown
## Python support

Python repos are labeled via `tree-sitter-python 0.25` (SPEC §13.1 pin). Symbol
resolution is a tree-sitter scope walker + lexical import graph — no Python
runtime is required.

Usage:
```bash
provbench-labeler run         --repo path/to/python/repo --t0 <sha> --out corpus.jsonl
provbench-labeler emit-facts  --repo path/to/python/repo --t0 <sha> --out facts.jsonl
provbench-labeler emit-diffs  --repo path/to/python/repo --t0 <sha> --out-dir diffs/
provbench-labeler spotcheck   --lang python --corpus corpus.jsonl --out spotcheck.csv --n 200 --seed 13897750829054410479
```

Known limitations:
- Star-imports re-export only via `__all__` (if present).
- Dynamic dispatch, `TYPE_CHECKING`-conditional imports, metaclass attribute generation: not modeled.

Determinism is enforced by `tests/determinism_python.rs` (fixture) and
`tests/determinism_flask.rs` (full flask, `#[ignore]` — opt-in via
`cargo test -- --ignored`).
```

- [ ] **Step 2: Commit**

```bash
git add benchmarks/provbench/labeler/README.md
git commit -m "docs(provbench-labeler): document Python usage + grammar pin"
```

---

## Task 21: Open PR

**Files:**
- External: GitHub PR against `main`

- [ ] **Step 1: Push branch + open PR**

```bash
git push -u origin feat/provbench-v1.2b-python-labeler
gh pr create --base main \
  --title "feat(provbench-labeler): Python support for held-out flask evaluation" \
  --body "$(cat <<'EOF'
## Summary
- Adds Python labeling to provbench-labeler via tree-sitter-python 0.25 (SPEC §13.1 pin).
- Pure-Rust extension; no Python runtime. Tree-sitter scope walker + lexical import graph implements SymbolResolver.
- Rust labeling path byte-stable across this change (canary asserted at Tasks 3, 12, and 18).

## Acceptance gates
- [ ] ripgrep canary corpus SHA unchanged vs pre-change
- [ ] fixture-level Python determinism (corpus/facts/diffs)
- [ ] full-flask Python determinism (`cargo test -- --ignored`)
- [ ] §9.1 spot-check on flask: Wilson 95% LB ≥ 0.95 (n=200)
- [ ] cargo fmt + clippy -D warnings clean

## Out of scope
- Held-out evaluation on flask (Plan B: `docs/superpowers/plans/2026-05-15-provbench-v1.2b-flask-heldout.md`)
- Phase 1 rule retuning (v1.2 byte-frozen per SPEC §10)

## Test plan
- [ ] CI green on the Plan A branch
- [ ] Reviewer reproduces the §9.1 spot-check from the committed CSV + findings
- [ ] Reviewer verifies the canary SHA file matches a fresh `provbench-labeler run` on ripgrep with `jq -c 'del(.labeler_git_sha)' | sha256sum` applied
EOF
)"
```

- [ ] **Step 2: After merge, record the merged SHA for Plan B**

Plan B's "Frozen pins" section requires this plan's merged commit SHA on `main`. Record it in:
```bash
git rev-parse origin/main > /tmp/python-labeler-merged-sha.txt
cat /tmp/python-labeler-merged-sha.txt
```
Write that SHA into Plan B's "Labeler git SHA (Python path)" frozen pin before Plan B execution begins.

---

## Self-Review checklist (run before declaring Plan A complete)

1. **Spec coverage:**
   - SPEC §13.1 tree-sitter-python pin → Task 4
   - SPEC §57 "tree-sitter + import graph for Python" → Tasks 5 (AST), 11 (resolver)
   - SPEC §9.1 spot-check ≥0.95 gate → Task 19
   - SPEC §10 anti-leakage (no phase1 rule changes) → confirmed by "Non-goals"
   - SPEC §13.2 held-out #2 pin → Task 16

2. **Placeholder scan:** None of `TBD`, `TODO`, `implement later`, `similar to Task N` in this plan. Two intentional `todo!()` macros live inside Task 11 Step 2's code skeleton — execution must replace them.

3. **Type consistency:** `Language`, `Fact`, `FactKind`, `SymbolResolver`, `ResolvedLocation`, `PythonAst`, `PythonResolver` used identically across all tasks.

4. **No code-style violations:** All new modules use `BTreeMap` for any persisted iteration, sort directory walks, no `HashMap` in serialization paths. Reviewer should grep for `HashMap` in `src/facts/python/` and `src/resolve/python.rs` before merge — none should appear.

---

## Execution handoff

**Plan complete and saved to `docs/superpowers/plans/2026-05-15-provbench-v1.2b-python-labeler.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — Dispatch a fresh subagent per task; review between tasks. Fast iteration.

**2. Inline Execution** — Execute tasks in this session using `superpowers:executing-plans`; batch with checkpoints.

When ready to execute, switch to a fresh branch off `main`:
```bash
git checkout main && git pull --ff-only
git checkout -b feat/provbench-v1.2b-python-labeler
```
