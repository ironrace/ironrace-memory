# ProvBench Phase 0b — Pilot Corpus Labeler Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a deterministic mechanical labeler that produces a versioned `fact_at_commit` corpus for the ripgrep pilot repo, satisfying the SPEC.md §9.1 acceptance gates (≥95% spot-check on 200 samples + byte-identical re-run).

**Architecture:** Standalone Cargo package at `benchmarks/provbench/labeler/` (excluded from the workspace per the locked plan: "Phase 0 lives outside any crate"). Tree-sitter extracts structural facts from Rust ASTs at observation time T₀; per-commit replay applies the §5 labeling rules using tree-sitter for span hashing, `rust-analyzer` LSP for symbol resolution, and the `similar` crate for rename detection. Output is JSONL (one `fact_at_commit` row per line) under `benchmarks/provbench/corpus/`, stamped with the labeler's git SHA. A separate spot-check sampler emits a CSV with the labels hidden for human review and a Wilson 95% CI report.

**Tech Stack:** Rust 1.91 (edition 2021), `tree-sitter` 0.25.6 + `tree-sitter-rust` 0.24.0, `rust-analyzer` 1.85.0 (external binary, LSP stdio), `gix` (pure-Rust git), `similar` (Myers diff), `pulldown-cmark` (README parsing), `serde_json`, `sha2`, `clap`, `anyhow`, `thiserror`, `tracing`.

**Non-goals:**
- Phase 0c LLM baseline (separate plan).
- Python (held-out repo `flask`) — labeler must be Python-ready architecturally, but Phase 0b only exercises the Rust path.
- Anything in `crates/ironmem/` — Phase 0 is intentionally outside the system code.

---

## File Map

Files created/modified during this plan, grouped by responsibility. Each file ≤400 lines unless noted.

```
Cargo.toml                                              [modify]  exclude benchmarks/provbench/labeler from workspace
benchmarks/provbench/labeler/
  Cargo.toml                                            [create]  standalone package
  README.md                                             [create]  reproducibility instructions
  src/
    main.rs                                             [create]  clap CLI entry point
    lib.rs                                              [create]  re-exports
    config.rs                                           [create]  paths, fixed seeds, defaults
    tooling.rs                                          [create]  rust-analyzer + tree-sitter binary discovery + hash verification
    repo.rs                                             [create]  gix-based pilot repo clone + commit walking
    ast/
      mod.rs                                            [create]  tree-sitter Rust parser wrapper
      spans.rs                                          [create]  byte-span + line-span helpers, content hashing
    facts/
      mod.rs                                            [create]  Fact enum + serde
      function_signature.rs                             [create]  extractor #1
      field.rs                                          [create]  extractor #2 (struct/enum field)
      symbol_existence.rs                               [create]  extractor #3
      doc_claim.rs                                      [create]  extractor #4 (README/doc → resolvable symbol)
      test_assertion.rs                                 [create]  extractor #5
    resolve/
      mod.rs                                            [create]  SymbolResolver trait
      rust_analyzer.rs                                  [create]  LSP stdio client (spawn, initialize, workspace/symbol)
    diff/
      mod.rs                                            [create]  whitespace/comment-only detector + rename detector
    label.rs                                            [create]  labeling rule engine (§5 of SPEC)
    replay.rs                                           [create]  per-commit driver
    output.rs                                           [create]  JSONL emitter, labeler-SHA stamping
    spotcheck.rs                                        [create]  stratified sampler + Wilson CI report
  tests/
    determinism.rs                                      [create]  re-run produces byte-identical output
    fixtures/                                           [create]  small synthetic git repo for unit tests (NOT ripgrep)
.gitignore                                              [modify]  exclude benchmarks/provbench/work/ (clone target)
benchmarks/provbench/corpus/.gitkeep                    [create]  populated by pilot run
benchmarks/provbench/spotcheck/.gitkeep                 [create]  populated by sampler
```

---

### Task 1: Bootstrap labeler package

**Files:**
- Create: `benchmarks/provbench/labeler/Cargo.toml`
- Create: `benchmarks/provbench/labeler/src/main.rs`
- Create: `benchmarks/provbench/labeler/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)
- Modify: `.gitignore`

- [ ] **Step 1: Write the failing test**

Create `benchmarks/provbench/labeler/tests/smoke.rs`:

```rust
#[test]
fn binary_prints_version_when_invoked_with_dash_v() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_provbench-labeler"))
        .arg("--version")
        .output()
        .expect("run labeler");
    assert!(out.status.success(), "labeler --version exited non-zero: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("provbench-labeler"), "missing name: {stdout}");
}
```

- [ ] **Step 2: Run test to verify it fails (binary doesn't exist yet)**

Run: `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml`
Expected: FAIL with manifest-not-found or build failure.

- [ ] **Step 3: Add workspace exclusion in root `Cargo.toml`**

Add to root `Cargo.toml` under `[workspace]`:

```toml
exclude = ["benchmarks/provbench/labeler"]
```

- [ ] **Step 4: Create `benchmarks/provbench/labeler/Cargo.toml`**

```toml
[package]
name = "provbench-labeler"
version = "0.1.0"
edition = "2021"
rust-version = "1.91"
description = "Phase 0b mechanical labeler for ProvBench-CodeContext (frozen contract: benchmarks/provbench/SPEC.md)"
license = "Apache-2.0"
publish = false

[[bin]]
name = "provbench-labeler"
path = "src/main.rs"

[lib]
name = "provbench_labeler"
path = "src/lib.rs"

[dependencies]
anyhow = "1"
thiserror = "2"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
gix = { version = "0.66", default-features = false, features = ["blocking-network-client", "max-performance-safe"] }
tree-sitter = "0.25"
tree-sitter-rust = "0.24"
similar = "2"
pulldown-cmark = { version = "0.12", default-features = false }
rand = "0.8"
rand_chacha = "0.3"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 5: Create `benchmarks/provbench/labeler/src/main.rs`**

```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "provbench-labeler", version, about = "ProvBench Phase 0b labeler")]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// Print the labeler git SHA stamp used for output rows.
    Stamp,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    match cli.command {
        None => Ok(()),
        Some(Cmd::Stamp) => {
            println!("{}", provbench_labeler::labeler_stamp());
            Ok(())
        }
    }
}
```

- [ ] **Step 6: Create `benchmarks/provbench/labeler/src/lib.rs`**

```rust
//! ProvBench Phase 0b mechanical labeler.
//!
//! Frozen contract: `benchmarks/provbench/SPEC.md`. This crate does not
//! depend on `ironmem` or any workspace crate — Phase 0 lives outside the
//! system code so the corpus and labeler can be released as a standalone
//! reproducible artifact.

pub fn labeler_stamp() -> String {
    option_env!("PROVBENCH_LABELER_GIT_SHA")
        .unwrap_or("unstamped")
        .to_string()
}
```

- [ ] **Step 7: Add gitignore entry**

Append to `.gitignore`:

```
benchmarks/provbench/work/
```

- [ ] **Step 8: Run tests and verify they pass**

Run: `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml`
Expected: PASS (1 test, smoke).

Run: `cargo fmt --manifest-path benchmarks/provbench/labeler/Cargo.toml --all -- --check`
Run: `cargo clippy --manifest-path benchmarks/provbench/labeler/Cargo.toml --all-targets --all-features -- -D warnings`
Expected: both succeed.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml .gitignore benchmarks/provbench/labeler/
git commit -m "feat(provbench): bootstrap phase 0b labeler crate"
```

---

### Task 2: Tooling pin verification

**Files:**
- Create: `benchmarks/provbench/labeler/src/tooling.rs`
- Modify: `benchmarks/provbench/labeler/src/lib.rs` (add `pub mod tooling;`)
- Modify: `benchmarks/provbench/labeler/src/main.rs` (add `verify-tooling` subcommand)

The SPEC §13.1 pins `rust-analyzer 1.85.0 (4d91de4e 2025-02-17)` with content hash `f85740bfa5b9136e9053768c015c31a6c7556f7cfe44f7f9323965034e1f9aee` and `tree-sitter 0.25.6` at `/opt/homebrew/bin/tree-sitter` with hash `3e82f0982232f68fd5b0192caf4bb06064cc034f837552272eec8d67014edc5c`. Phase 0b labels are invalid unless the binaries match these hashes.

- [ ] **Step 1: Write the failing test**

Create `benchmarks/provbench/labeler/tests/tooling.rs`:

```rust
use provbench_labeler::tooling::{ExpectedTool, verify_binary_hash};
use std::io::Write;

#[test]
fn rejects_binary_with_wrong_hash() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(b"not the real binary").unwrap();
    let path = tmp.path().to_path_buf();
    let expected = ExpectedTool {
        name: "fake",
        version_hint: "n/a",
        sha256_hex: "0000000000000000000000000000000000000000000000000000000000000000",
    };
    let err = verify_binary_hash(&path, &expected).unwrap_err();
    assert!(err.to_string().contains("hash mismatch"), "unexpected err: {err}");
}

#[test]
fn accepts_binary_when_hash_matches() {
    use sha2::{Digest, Sha256};
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    let bytes = b"hello world";
    tmp.write_all(bytes).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hex = format!("{:x}", hasher.finalize());
    let expected = ExpectedTool {
        name: "fake",
        version_hint: "n/a",
        sha256_hex: Box::leak(hex.into_boxed_str()),
    };
    verify_binary_hash(tmp.path(), &expected).expect("hash should match");
}
```

- [ ] **Step 2: Run test to verify it fails (module doesn't exist)**

Run: `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml --test tooling`
Expected: FAIL — `unresolved module`.

- [ ] **Step 3: Implement `tooling.rs`**

```rust
//! Tooling-pin verification per SPEC §13.1.
//!
//! Phase 0b labels are invalid unless every external tool used at label
//! time matches the binary content hash recorded in the spec freeze. A
//! version-string match alone is **not** sufficient — distros patch.

use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct ExpectedTool {
    pub name: &'static str,
    pub version_hint: &'static str,
    pub sha256_hex: &'static str,
}

/// rust-analyzer 1.85.0 (4d91de4e 2025-02-17), rustup stable-aarch64-apple-darwin.
pub const RUST_ANALYZER: ExpectedTool = ExpectedTool {
    name: "rust-analyzer",
    version_hint: "1.85.0 (4d91de4e 2025-02-17)",
    sha256_hex: "f85740bfa5b9136e9053768c015c31a6c7556f7cfe44f7f9323965034e1f9aee",
};

/// tree-sitter 0.25.6 (Homebrew, /opt/homebrew/bin/tree-sitter).
pub const TREE_SITTER: ExpectedTool = ExpectedTool {
    name: "tree-sitter",
    version_hint: "0.25.6",
    sha256_hex: "3e82f0982232f68fd5b0192caf4bb06064cc034f837552272eec8d67014edc5c",
};

pub fn verify_binary_hash(path: &Path, expected: &ExpectedTool) -> Result<()> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read {} at {}", expected.name, path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected.sha256_hex {
        return Err(anyhow!(
            "tooling hash mismatch for {}: expected {} (version {}), got {}",
            expected.name,
            expected.sha256_hex,
            expected.version_hint,
            actual
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ResolvedTooling {
    pub rust_analyzer: std::path::PathBuf,
    pub tree_sitter: std::path::PathBuf,
}

pub fn resolve_from_env() -> Result<ResolvedTooling> {
    let rust_analyzer = which::which("rust-analyzer")
        .or_else(|_| -> Result<_, which::Error> {
            Ok(std::path::PathBuf::from("/opt/homebrew/bin/rust-analyzer"))
        })
        .context("rust-analyzer must be on PATH or at /opt/homebrew/bin/rust-analyzer")?;
    let tree_sitter = std::path::PathBuf::from("/opt/homebrew/bin/tree-sitter");
    verify_binary_hash(&rust_analyzer, &RUST_ANALYZER)?;
    verify_binary_hash(&tree_sitter, &TREE_SITTER)?;
    Ok(ResolvedTooling { rust_analyzer, tree_sitter })
}
```

Add `which = "6"` to `[dependencies]` in `benchmarks/provbench/labeler/Cargo.toml`.

- [ ] **Step 4: Wire into `lib.rs` and add CLI subcommand**

`benchmarks/provbench/labeler/src/lib.rs` becomes:

```rust
//! ProvBench Phase 0b mechanical labeler.
pub mod tooling;

pub fn labeler_stamp() -> String {
    option_env!("PROVBENCH_LABELER_GIT_SHA")
        .unwrap_or("unstamped")
        .to_string()
}
```

`benchmarks/provbench/labeler/src/main.rs` `Cmd` enum gains:

```rust
    /// Verify pinned external tools match SPEC §13.1 content hashes.
    VerifyTooling,
```

and the match arm:

```rust
        Some(Cmd::VerifyTooling) => {
            let resolved = provbench_labeler::tooling::resolve_from_env()?;
            println!("rust-analyzer: {}", resolved.rust_analyzer.display());
            println!("tree-sitter:  {}", resolved.tree_sitter.display());
            Ok(())
        }
```

- [ ] **Step 5: Verify tests pass**

Run: `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml`
Expected: PASS (3 tests).

Run: `cargo fmt … --check && cargo clippy … -D warnings`
Expected: both succeed.

- [ ] **Step 6: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): pin rust-analyzer and tree-sitter binary hashes"
```

---

### Task 3: Pilot repo cloning + commit walking

**Files:**
- Create: `benchmarks/provbench/labeler/src/repo.rs`
- Modify: `benchmarks/provbench/labeler/src/lib.rs` (add `pub mod repo;`)

Pilot repo per SPEC §13.2: `https://github.com/BurntSushi/ripgrep` at T₀ commit `af6b6c543b224d348a8876f0c06245d9ea7929c5`. Clone target: `benchmarks/provbench/work/ripgrep/` (gitignored). Commit walking iterates from T₀ forward in first-parent linear order.

- [ ] **Step 1: Write the failing test**

Create `benchmarks/provbench/labeler/tests/repo.rs`:

```rust
use provbench_labeler::repo::{Pilot, PilotRepoSpec};
use std::path::PathBuf;

fn fixture_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_path_buf();
    let status = std::process::Command::new("git")
        .args(["init", "--initial-branch=main", path.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    let write_and_commit = |name: &str, body: &str, msg: &str| {
        std::fs::write(path.join(name), body).unwrap();
        let s1 = std::process::Command::new("git")
            .args(["-C", path.to_str().unwrap(), "add", name])
            .status()
            .unwrap();
        assert!(s1.success());
        let s2 = std::process::Command::new("git")
            .args([
                "-C", path.to_str().unwrap(),
                "-c", "user.name=t", "-c", "user.email=t@t",
                "commit", "-m", msg,
            ])
            .status()
            .unwrap();
        assert!(s2.success());
    };
    write_and_commit("a.rs", "fn one() {}\n", "c1");
    write_and_commit("a.rs", "fn one_renamed() {}\n", "c2");
    write_and_commit("a.rs", "fn one_renamed() { let x = 1; }\n", "c3");
    (tmp, path)
}

#[test]
fn walks_first_parent_from_t0() {
    let (_keep, path) = fixture_repo();
    let head = std::process::Command::new("git")
        .args(["-C", path.to_str().unwrap(), "rev-list", "--max-parents=0", "HEAD"])
        .output()
        .unwrap();
    let t0 = String::from_utf8(head.stdout).unwrap().trim().to_string();
    let pilot = Pilot::open(&PilotSpecLocal { path: path.clone(), t0_sha: t0.clone() }).unwrap();
    let commits: Vec<_> = pilot.walk_first_parent().unwrap().collect();
    assert_eq!(commits.len(), 3, "got {commits:?}");
    assert_eq!(commits[0].sha, t0);
}

struct PilotSpecLocal {
    path: PathBuf,
    t0_sha: String,
}

impl PilotRepoSpec for PilotSpecLocal {
    fn local_clone_path(&self) -> &std::path::Path {
        &self.path
    }
    fn t0_sha(&self) -> &str {
        &self.t0_sha
    }
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml --test repo`
Expected: FAIL.

- [ ] **Step 3: Implement `repo.rs`**

```rust
//! Pilot repo handling: open an already-cloned repo and walk first-parent
//! commits from T₀ forward. Cloning itself is left to a CLI step (or to
//! the user) — the labeler refuses to mutate git state.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};

/// Pinned pilot repo per SPEC §13.2.
pub const RIPGREP_T0_SHA: &str = "af6b6c543b224d348a8876f0c06245d9ea7929c5";
pub const RIPGREP_URL: &str = "https://github.com/BurntSushi/ripgrep";

pub trait PilotRepoSpec {
    fn local_clone_path(&self) -> &Path;
    fn t0_sha(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct Ripgrep {
    pub clone_path: PathBuf,
}

impl PilotRepoSpec for Ripgrep {
    fn local_clone_path(&self) -> &Path {
        &self.clone_path
    }
    fn t0_sha(&self) -> &str {
        RIPGREP_T0_SHA
    }
}

#[derive(Debug, Clone)]
pub struct CommitRef {
    pub sha: String,
    pub parent_sha: Option<String>,
}

pub struct Pilot {
    repo: gix::Repository,
    t0_sha: gix::ObjectId,
}

impl Pilot {
    pub fn open<S: PilotRepoSpec>(spec: &S) -> Result<Self> {
        let repo = gix::open(spec.local_clone_path())
            .with_context(|| format!("open pilot repo at {}", spec.local_clone_path().display()))?;
        let t0_sha = gix::ObjectId::from_hex(spec.t0_sha().as_bytes())
            .with_context(|| format!("parse t0 sha {}", spec.t0_sha()))?;
        repo.find_object(t0_sha)
            .with_context(|| format!("t0 commit {} not present in clone", spec.t0_sha()))?;
        Ok(Self { repo, t0_sha })
    }

    /// Walk first-parent linear history from T₀ forward up to HEAD.
    /// Returns commits in chronological order (oldest first).
    pub fn walk_first_parent(&self) -> Result<impl Iterator<Item = CommitRef> + '_> {
        let head = self.repo.head_commit().context("resolve HEAD")?;
        let mut chain: Vec<gix::ObjectId> = Vec::new();
        let mut cur = Some(head.id);
        while let Some(id) = cur {
            chain.push(id);
            if id == self.t0_sha {
                break;
            }
            let commit = self.repo.find_commit(id)
                .with_context(|| format!("walk: find {id}"))?;
            cur = commit.parent_ids().next().map(|p| p.detach());
        }
        if chain.last() != Some(&self.t0_sha) {
            return Err(anyhow!(
                "first-parent chain from HEAD does not contain T₀ {}; rebased history?",
                self.t0_sha
            ));
        }
        chain.reverse();
        let repo = &self.repo;
        Ok(chain.into_iter().map(move |id| {
            let parent = repo
                .find_commit(id)
                .ok()
                .and_then(|c| c.parent_ids().next().map(|p| p.detach().to_string()));
            CommitRef { sha: id.to_string(), parent_sha: parent }
        }))
    }

    pub fn read_blob_at(&self, commit_sha: &str, path: &Path) -> Result<Option<Vec<u8>>> {
        let id = gix::ObjectId::from_hex(commit_sha.as_bytes())?;
        let commit = self.repo.find_commit(id)?;
        let tree = commit.tree()?;
        let entry = tree.lookup_entry_by_path(path, &mut Vec::new())?;
        match entry {
            None => Ok(None),
            Some(e) => {
                let obj = self.repo.find_object(e.oid())?;
                Ok(Some(obj.data.clone()))
            }
        }
    }
}
```

- [ ] **Step 4: Verify tests pass**

Run: `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml`
Expected: PASS (4 tests now).

Run fmt + clippy gates.

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): pilot repo open + first-parent commit walk"
```

---

### Task 4: Tree-sitter Rust parser + span helpers

**Files:**
- Create: `benchmarks/provbench/labeler/src/ast/mod.rs`
- Create: `benchmarks/provbench/labeler/src/ast/spans.rs`
- Modify: `benchmarks/provbench/labeler/src/lib.rs` (add `pub mod ast;`)

- [ ] **Step 1: Write failing test**

Create `benchmarks/provbench/labeler/src/ast/mod.rs` (empty stub) and add inline tests at the bottom of `spans.rs` once written. First write `tests/ast.rs`:

```rust
use provbench_labeler::ast::{RustAst, spans::{Span, content_hash}};

#[test]
fn parses_function_and_returns_signature_span() {
    let src = b"fn add(a: i32, b: i32) -> i32 { a + b }\n";
    let ast = RustAst::parse(src).unwrap();
    let fns: Vec<_> = ast.function_signature_spans().collect();
    assert_eq!(fns.len(), 1);
    let (name, span) = &fns[0];
    assert_eq!(name, "add");
    let bytes = &src[span.byte_range.clone()];
    let text = std::str::from_utf8(bytes).unwrap();
    assert!(text.starts_with("fn add"));
    assert!(text.ends_with("-> i32"), "got: {text}");
}

#[test]
fn content_hash_is_stable_for_same_bytes() {
    let h1 = content_hash(b"fn x() {}");
    let h2 = content_hash(b"fn x() {}");
    assert_eq!(h1, h2);
    assert_ne!(h1, content_hash(b"fn y() {}"));
    assert_eq!(h1.len(), 64);
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test … --test ast`
Expected: FAIL — `unresolved module`.

- [ ] **Step 3: Implement `spans.rs`**

```rust
//! Byte-span + line-span types and content hashing used by every fact
//! kind. SHA-256 is used everywhere — never `Hash`/`u64` — so labels are
//! reproducible across runs and machines.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub byte_range: std::ops::Range<usize>,
    pub line_start: u32, // 1-based inclusive
    pub line_end: u32,   // 1-based inclusive
}

pub fn content_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// Hash only the slice within `span` from `source`. Convenience for the
/// labeling rule engine.
pub fn span_hash(source: &[u8], span: &Span) -> String {
    content_hash(&source[span.byte_range.clone()])
}
```

- [ ] **Step 4: Implement `ast/mod.rs`**

```rust
//! Tree-sitter Rust parser wrapper. Owns the parser handle and tree;
//! offers high-level iterators per fact kind. Python support will be
//! added in a sibling module — keep this Rust-only.

pub mod spans;

use anyhow::{Context, Result};
use spans::Span;
use tree_sitter::{Node, Parser, Tree};

pub struct RustAst {
    src: Vec<u8>,
    tree: Tree,
}

impl RustAst {
    pub fn parse(src: &[u8]) -> Result<Self> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .context("set rust language")?;
        let tree = parser
            .parse(src, None)
            .ok_or_else(|| anyhow::anyhow!("tree-sitter returned no tree"))?;
        Ok(Self { src: src.to_vec(), tree })
    }

    pub fn source(&self) -> &[u8] {
        &self.src
    }

    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }

    /// Yield (function name, signature span) pairs. The signature span
    /// covers `fn NAME(...) -> R` and stops before the function body.
    pub fn function_signature_spans(&self) -> impl Iterator<Item = (String, Span)> + '_ {
        crate::facts::function_signature::iter(self)
    }
}

pub(crate) fn line_span_from_node(src: &[u8], node: Node<'_>) -> Span {
    Span {
        byte_range: node.start_byte()..node.end_byte(),
        line_start: (node.start_position().row + 1) as u32,
        line_end: (node.end_position().row + 1) as u32,
    }
}

#[allow(dead_code)]
pub(crate) fn line_span_through(src: &[u8], start: Node<'_>, end_byte: usize) -> Span {
    let line_end_row = src[..end_byte].iter().filter(|b| **b == b'\n').count() as u32 + 1;
    Span {
        byte_range: start.start_byte()..end_byte,
        line_start: (start.start_position().row + 1) as u32,
        line_end: line_end_row,
    }
}
```

- [ ] **Step 5: Stub `facts/function_signature.rs` minimally so the test passes**

Create `benchmarks/provbench/labeler/src/facts/mod.rs`:

```rust
pub mod function_signature;
```

Add `pub mod facts;` to `lib.rs`.

Create `benchmarks/provbench/labeler/src/facts/function_signature.rs` with **only the iter helper**:

```rust
//! Function-signature fact extractor. Walks a Rust tree-sitter tree and
//! yields (qualified-or-bare name, signature-span) pairs. The signature
//! span ends immediately before the function body — body changes alone
//! are NOT a signature change.

use crate::ast::{RustAst, line_span_through, spans::Span};
use tree_sitter::Node;

pub fn iter(ast: &RustAst) -> impl Iterator<Item = (String, Span)> + '_ {
    let mut out = Vec::new();
    let src = ast.source();
    let root = ast.root();
    walk(root, src, &mut out);
    out.into_iter()
}

fn walk(node: Node<'_>, src: &[u8], out: &mut Vec<(String, Span)>) {
    if node.kind() == "function_item" {
        if let Some((name, span)) = extract_signature(node, src) {
            out.push((name, span));
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, out);
    }
}

fn extract_signature(node: Node<'_>, src: &[u8]) -> Option<(String, Span)> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(src).ok()?.to_string();
    let body = node.child_by_field_name("body");
    let sig_end_byte = body
        .map(|b| b.start_byte())
        .unwrap_or_else(|| node.end_byte());
    let span = line_span_through(src, node, sig_end_byte);
    Some((name, span))
}
```

- [ ] **Step 6: Verify tests pass**

Run all tests + fmt + clippy.
Expected: PASS (6 tests).

- [ ] **Step 7: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): tree-sitter Rust AST + span helpers + signature extractor scaffold"
```

---

### Task 5: Function-signature fact extractor (full)

**Files:**
- Modify: `benchmarks/provbench/labeler/src/facts/function_signature.rs`
- Modify: `benchmarks/provbench/labeler/src/facts/mod.rs` (add `Fact` enum)

The signature span must include attributes/visibility (`pub`, `#[inline]`) so a visibility change registers as a stale signature. Module-qualified path is computed by walking parent `mod` nodes.

- [ ] **Step 1: Write failing test**

Append to `tests/ast.rs`:

```rust
use provbench_labeler::facts::Fact;
use provbench_labeler::facts::function_signature;

#[test]
fn signature_includes_visibility_and_attrs() {
    let src = b"#[inline]\npub fn add(a: i32) -> i32 { a }\n";
    let ast = provbench_labeler::ast::RustAst::parse(src).unwrap();
    let facts: Vec<_> = function_signature::extract(&ast, std::path::Path::new("a.rs")).collect();
    assert_eq!(facts.len(), 1);
    match &facts[0] {
        Fact::FunctionSignature { qualified_name, span, content_hash, source_path } => {
            assert_eq!(qualified_name, "add");
            assert_eq!(source_path, std::path::Path::new("a.rs"));
            let body = &src[span.byte_range.clone()];
            assert!(body.starts_with(b"#[inline]"));
            assert!(content_hash.len() == 64);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn nested_module_qualified_name() {
    let src = b"mod a { mod b { pub fn deep() {} } }\n";
    let ast = provbench_labeler::ast::RustAst::parse(src).unwrap();
    let facts: Vec<_> = function_signature::extract(&ast, std::path::Path::new("lib.rs")).collect();
    assert_eq!(facts.len(), 1);
    match &facts[0] {
        Fact::FunctionSignature { qualified_name, .. } => {
            assert_eq!(qualified_name, "a::b::deep");
        }
        _ => panic!(),
    }
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL — `extract` doesn't exist, `Fact` doesn't exist.

- [ ] **Step 3: Define `Fact` enum in `facts/mod.rs`**

```rust
//! Closed enum of fact kinds (SPEC §3.1). Adding a kind is a §11 spec
//! change — do not extend silently.

pub mod function_signature;

use crate::ast::spans::Span;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Fact {
    FunctionSignature {
        qualified_name: String,
        source_path: PathBuf,
        span: Span,
        content_hash: String,
    },
}
```

- [ ] **Step 4: Implement `extract` in `function_signature.rs`**

Replace the file with the version that walks the AST, builds the qualified module path by tracking ancestor `mod_item` names, and emits `Fact::FunctionSignature` instead of the bare tuple. Keep `iter` as a thin wrapper used by `RustAst::function_signature_spans`. Hash the bytes of the signature span to produce `content_hash`.

```rust
use crate::ast::{RustAst, line_span_through, spans::{Span, content_hash}};
use crate::facts::Fact;
use std::path::Path;
use tree_sitter::Node;

pub fn iter(ast: &RustAst) -> impl Iterator<Item = (String, Span)> + '_ {
    extract(ast, Path::new(""))
        .filter_map(|f| match f {
            Fact::FunctionSignature { qualified_name, span, .. } => Some((qualified_name, span)),
        })
        .collect::<Vec<_>>()
        .into_iter()
}

pub fn extract<'a>(ast: &'a RustAst, source_path: &'a Path) -> impl Iterator<Item = Fact> + 'a {
    let mut out = Vec::new();
    let src = ast.source();
    let root = ast.root();
    walk(root, src, &[], source_path, &mut out);
    out.into_iter()
}

fn walk(node: Node<'_>, src: &[u8], mod_path: &[String], source_path: &Path, out: &mut Vec<Fact>) {
    let kind = node.kind();
    if kind == "function_item" {
        if let Some(fact) = extract_one(node, src, mod_path, source_path) {
            out.push(fact);
        }
    }
    if kind == "mod_item" {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(name) = name_node.utf8_text(src) {
                let mut next = mod_path.to_vec();
                next.push(name.to_string());
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    walk(child, src, &next, source_path, out);
                }
                return;
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, mod_path, source_path, out);
    }
}

fn extract_one(node: Node<'_>, src: &[u8], mod_path: &[String], source_path: &Path) -> Option<Fact> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(src).ok()?.to_string();
    let qualified_name = if mod_path.is_empty() {
        name
    } else {
        let mut s = mod_path.join("::");
        s.push_str("::");
        s.push_str(&name);
        s
    };
    let start_node = leading_attribute_or_self(node);
    let body = node.child_by_field_name("body");
    let sig_end_byte = body.map(|b| b.start_byte()).unwrap_or_else(|| node.end_byte());
    let span = line_span_through(src, start_node, sig_end_byte);
    let hash = content_hash(&src[span.byte_range.clone()]);
    Some(Fact::FunctionSignature {
        qualified_name,
        source_path: source_path.to_path_buf(),
        span,
        content_hash: hash,
    })
}

fn leading_attribute_or_self(node: Node<'_>) -> Node<'_> {
    let mut start = node;
    while let Some(prev) = start.prev_sibling() {
        match prev.kind() {
            "attribute_item" | "inner_attribute_item" | "line_comment" | "block_comment" => {
                start = prev;
            }
            _ => break,
        }
    }
    start
}
```

- [ ] **Step 5: Verify all tests pass + fmt + clippy**

- [ ] **Step 6: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): function-signature fact extractor with module-qualified names"
```

---

### Task 6: Struct/enum field fact extractor

**Files:**
- Create: `benchmarks/provbench/labeler/src/facts/field.rs`
- Modify: `benchmarks/provbench/labeler/src/facts/mod.rs` (add `Field` variant + `pub mod field;`)

Extracts `Fact::Field { qualified_path: "Type::field", source_path, span, content_hash, type_text }`. Each field gets its own fact (one row per field).

- [ ] **Step 1: Write failing test**

Append to `tests/ast.rs`:

```rust
use provbench_labeler::facts::field;

#[test]
fn struct_fields_each_emit_one_fact() {
    let src = b"pub struct Foo { pub a: u32, b: String }\n";
    let ast = provbench_labeler::ast::RustAst::parse(src).unwrap();
    let facts: Vec<_> = field::extract(&ast, std::path::Path::new("a.rs")).collect();
    assert_eq!(facts.len(), 2);
    let names: Vec<_> = facts.iter().map(|f| match f {
        Fact::Field { qualified_path, type_text, .. } => (qualified_path.clone(), type_text.clone()),
        _ => panic!(),
    }).collect();
    assert!(names.contains(&("Foo::a".into(), "u32".into())));
    assert!(names.contains(&("Foo::b".into(), "String".into())));
}

#[test]
fn enum_struct_variant_fields_qualified_with_variant() {
    let src = b"pub enum E { V { x: i32 } }\n";
    let ast = provbench_labeler::ast::RustAst::parse(src).unwrap();
    let facts: Vec<_> = field::extract(&ast, std::path::Path::new("a.rs")).collect();
    assert_eq!(facts.len(), 1);
    match &facts[0] {
        Fact::Field { qualified_path, .. } => assert_eq!(qualified_path, "E::V::x"),
        _ => panic!(),
    }
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL — `field` module + `Fact::Field` don't exist.

- [ ] **Step 3: Add variant + implement extractor**

In `facts/mod.rs`:

```rust
pub mod field;

…

#[serde(tag = "kind")]
pub enum Fact {
    FunctionSignature { … },
    Field {
        qualified_path: String,
        source_path: PathBuf,
        type_text: String,
        span: Span,
        content_hash: String,
    },
}
```

In `facts/field.rs`: walk `struct_item`, `enum_item` → `enum_variant` → `field_declaration_list` → `field_declaration`; emit one fact per field. Use `child_by_field_name("name")` and `child_by_field_name("type")` (tree-sitter-rust grammar). Strip surrounding whitespace from the type text (`name_node.utf8_text(src)?.trim()`).

Implementation pattern follows `function_signature.rs`. Keep ≤200 lines.

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): struct/enum field fact extractor"
```

---

### Task 7: Public symbol existence fact extractor

**Files:**
- Create: `benchmarks/provbench/labeler/src/facts/symbol_existence.rs`
- Modify: `benchmarks/provbench/labeler/src/facts/mod.rs` (add `PublicSymbol` variant)

Extracts `Fact::PublicSymbol { qualified_name, source_path, span, content_hash }` for every `pub` (non-`pub(crate)`, non-`pub(super)`) item: `fn`, `struct`, `enum`, `mod`, `trait`, `const`, `static`, `type`, `use` re-exports.

- [ ] **Step 1: Write failing test**

Append to `tests/ast.rs`:

```rust
use provbench_labeler::facts::symbol_existence;

#[test]
fn pub_items_emit_public_symbol_facts() {
    let src = b"pub fn f() {} pub struct S; pub(crate) fn private() {}\n";
    let ast = provbench_labeler::ast::RustAst::parse(src).unwrap();
    let facts: Vec<_> = symbol_existence::extract(&ast, std::path::Path::new("lib.rs")).collect();
    let names: Vec<_> = facts.iter().filter_map(|f| match f {
        Fact::PublicSymbol { qualified_name, .. } => Some(qualified_name.clone()),
        _ => None,
    }).collect();
    assert!(names.contains(&"f".to_string()));
    assert!(names.contains(&"S".to_string()));
    assert!(!names.contains(&"private".to_string()));
}

#[test]
fn pub_use_reexport_emits_symbol() {
    let src = b"mod m { pub fn inner() {} } pub use m::inner;\n";
    let ast = provbench_labeler::ast::RustAst::parse(src).unwrap();
    let facts: Vec<_> = symbol_existence::extract(&ast, std::path::Path::new("lib.rs")).collect();
    let names: Vec<_> = facts.iter().filter_map(|f| match f {
        Fact::PublicSymbol { qualified_name, .. } => Some(qualified_name.clone()),
        _ => None,
    }).collect();
    assert!(names.contains(&"inner".to_string()), "got {names:?}");
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL.

- [ ] **Step 3: Implement**

In `facts/mod.rs` add `PublicSymbol { qualified_name, source_path, span, content_hash }`.

In `facts/symbol_existence.rs`: walk node tree; for each item kind in {`function_item`, `struct_item`, `enum_item`, `mod_item`, `trait_item`, `const_item`, `static_item`, `type_item`}, check for an immediate `visibility_modifier` child whose first token is exactly `pub` (reject `pub(crate)` etc. by checking the visibility node's text equals `"pub"` after trimming). For `use_declaration` items with `pub` visibility, take the last identifier of each `scoped_use_list`/`use_as_clause` as the re-exported name.

Span = item header (visibility + keyword + name); for `use` it's the full statement.

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): public-symbol existence fact extractor"
```

---

### Task 8: README/doc claim fact extractor

**Files:**
- Create: `benchmarks/provbench/labeler/src/facts/doc_claim.rs`
- Modify: `benchmarks/provbench/labeler/src/facts/mod.rs` (add `DocClaim` variant)

A README/API-doc claim is a code-style mention of a symbol name (markdown inline code or fenced code block) that must back-resolve to a fact previously extracted in this commit's pass. Stores `Fact::DocClaim { doc_path, mention_span, mention_hash, defining_span, defining_hash, qualified_name }`.

Doc files in v1: `README.md` at the repo root + any `*.md` in the same directory as a Rust file. Restricting scope keeps the labeler tractable for the pilot.

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::facts::doc_claim;
use provbench_labeler::ast::RustAst;

#[test]
fn inline_code_mention_resolving_to_known_symbol_emits_doc_claim() {
    let rs = b"pub fn search() {}\n";
    let md = b"# rg\n\nUse `search` to scan files.\n";
    let ast = RustAst::parse(rs).unwrap();
    let known: Vec<_> = provbench_labeler::facts::symbol_existence::extract(
        &ast, std::path::Path::new("lib.rs")).collect();
    let claims: Vec<_> = doc_claim::extract(
        md, std::path::Path::new("README.md"), &known).collect();
    assert_eq!(claims.len(), 1);
    match &claims[0] {
        Fact::DocClaim { qualified_name, doc_path, .. } => {
            assert_eq!(qualified_name, "search");
            assert_eq!(doc_path, std::path::Path::new("README.md"));
        }
        _ => panic!(),
    }
}

#[test]
fn unresolvable_mention_is_not_emitted() {
    let rs = b"pub fn search() {}\n";
    let md = b"`nonexistent` is great.\n";
    let ast = RustAst::parse(rs).unwrap();
    let known: Vec<_> = provbench_labeler::facts::symbol_existence::extract(
        &ast, std::path::Path::new("lib.rs")).collect();
    let claims: Vec<_> = doc_claim::extract(
        md, std::path::Path::new("README.md"), &known).collect();
    assert_eq!(claims.len(), 0);
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL.

- [ ] **Step 3: Implement using `pulldown-cmark`**

Walk `Event::Code(CowStr)` and `Event::Start(Tag::CodeBlock(_))` … `Event::End`; record byte offsets via `Parser::new_ext(...).into_offset_iter()`. For each `Code(s)` mention, search `known` for a fact whose qualified-name's last segment equals `s` (case-sensitive). On match, emit `DocClaim` with both the mention span and the matching fact's defining span/hash.

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): README doc-claim fact extractor (back-resolved)"
```

---

### Task 9: Test-assertion fact extractor

**Files:**
- Create: `benchmarks/provbench/labeler/src/facts/test_assertion.rs`
- Modify: `benchmarks/provbench/labeler/src/facts/mod.rs` (add `TestAssertion` variant)

A test assertion is a call to `assert!`, `assert_eq!`, or `assert_ne!` inside a function annotated `#[test]`. The fact captures: test fn name, assertion span (the macro invocation), assertion hash, and the qualified name of any symbol referenced within the macro args (resolved by simple identifier match against `known`).

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::facts::test_assertion;
use provbench_labeler::ast::RustAst;

#[test]
fn test_assertion_referencing_known_fn_emits_fact() {
    let src = b"pub fn add(a: i32, b: i32) -> i32 { a + b }\n#[test]\nfn t() { assert_eq!(add(1, 2), 3); }\n";
    let ast = RustAst::parse(src).unwrap();
    let known: Vec<_> = provbench_labeler::facts::function_signature::extract(
        &ast, std::path::Path::new("a.rs")).collect();
    let facts: Vec<_> = test_assertion::extract(&ast, std::path::Path::new("a.rs"), &known).collect();
    assert_eq!(facts.len(), 1);
    match &facts[0] {
        Fact::TestAssertion { test_fn, asserted_symbol, .. } => {
            assert_eq!(test_fn, "t");
            assert_eq!(asserted_symbol.as_deref(), Some("add"));
        }
        _ => panic!(),
    }
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL.

- [ ] **Step 3: Implement**

Walk `function_item` nodes whose `attribute_item` ancestors include `#[test]` (string-match the attribute path). Inside, walk `macro_invocation` nodes; if the macro name is one of `assert`, `assert_eq`, `assert_ne`, extract the span. Then walk the macro's `token_tree` for identifier tokens and pick the first whose text matches a known fact's qualified-name last segment; that becomes `asserted_symbol`. Emit one `Fact::TestAssertion` per assertion.

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): test-assertion fact extractor"
```

---

### Task 10: rust-analyzer LSP client

**Files:**
- Create: `benchmarks/provbench/labeler/src/resolve/mod.rs`
- Create: `benchmarks/provbench/labeler/src/resolve/rust_analyzer.rs`
- Modify: `benchmarks/provbench/labeler/src/lib.rs` (add `pub mod resolve;`)

`SymbolResolver::resolve(qualified_name) -> Option<ResolvedLocation>` for a checked-out worktree. Implementation spawns rust-analyzer over stdio, sends `initialize` + `initialized`, then `workspace/symbol` queries; reuses one process per commit for performance. Adds `lsp-types = "0.97"` to the manifest.

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::resolve::SymbolResolver;
use provbench_labeler::resolve::rust_analyzer::RustAnalyzer;

#[test]
#[ignore = "requires rust-analyzer on PATH; run with `cargo test -- --ignored`"]
fn resolves_pub_fn_in_minimal_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"),
        b"[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
    std::fs::create_dir(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), b"pub fn marker_fn() {}\n").unwrap();
    let mut ra = RustAnalyzer::spawn(tmp.path()).unwrap();
    let resolved = ra.resolve("marker_fn").unwrap();
    assert!(resolved.is_some(), "marker_fn should resolve");
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL — module + types don't exist. (The `#[ignore]` keeps it off the default suite; integration tests gate it explicitly.)

- [ ] **Step 3: Implement `resolve/mod.rs`**

```rust
//! Symbol resolution traits. Phase 0b uses rust-analyzer for Rust;
//! Python (held-out) will get a tree-sitter + import-graph implementation
//! later — keep the trait language-agnostic.

pub mod rust_analyzer;

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLocation {
    pub file: PathBuf,
    pub line: u32,
}

pub trait SymbolResolver {
    fn resolve(&mut self, qualified_name: &str) -> anyhow::Result<Option<ResolvedLocation>>;
}
```

- [ ] **Step 4: Implement `resolve/rust_analyzer.rs`**

LSP stdio framing: read `Content-Length: N\r\n\r\n` then N bytes; write the same. Use `serde_json::Value` for messages plus `lsp-types` for typed payloads. Sequence:

1. `initialize` with `rootUri = file://$WORKSPACE`.
2. `initialized` notification.
3. Wait for `workspace/symbol` to be servable: poll `$/progress` "rustAnalyzer/Indexing" `kind: "end"` OR a 30s timeout.
4. `workspace/symbol` with the bare name (last segment of qualified_name); filter responses by exact qualified-name match.

Process is held in a struct with `Child`, `BufReader<ChildStdout>`, `ChildStdin`, and a request id counter. `Drop` sends `shutdown` + `exit`.

Keep the file ≤350 lines. Comprehensive error handling — every parse failure must surface a context message naming the LSP method.

- [ ] **Step 5: Verify**

Run unit tests (the `#[ignore]`d test stays off): `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml`.
Then run gated: `cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml -- --ignored`.
Both should pass on a machine with rust-analyzer installed; the gated test is required green before commit.
Run fmt + clippy.

- [ ] **Step 6: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): rust-analyzer LSP stdio client for symbol resolution"
```

---

### Task 11: Whitespace/comment-only diff detector

**Files:**
- Create: `benchmarks/provbench/labeler/src/diff/mod.rs`
- Modify: `benchmarks/provbench/labeler/src/lib.rs` (add `pub mod diff;`)

Per SPEC §5.3: `hash(span) != hash_at_observation` but the diff is whitespace-only or comment-only → `valid`. Implementation: tokenize before/after spans with tree-sitter, drop `comment` and whitespace tokens, compare the residual token streams.

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::diff::is_whitespace_or_comment_only;

#[test]
fn pure_whitespace_diff_is_ignored() {
    assert!(is_whitespace_or_comment_only(b"fn x()  {}", b"fn x() {}"));
}

#[test]
fn comment_only_diff_is_ignored() {
    assert!(is_whitespace_or_comment_only(b"fn x() {} // a", b"fn x() {} // b"));
}

#[test]
fn rename_is_not_whitespace_only() {
    assert!(!is_whitespace_or_comment_only(b"fn x() {}", b"fn y() {}"));
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
//! Per SPEC §5.3: whitespace-only or comment-only diffs do not invalidate
//! a fact even when content hashes differ. Implementation tokenizes both
//! sides with tree-sitter, drops trivia, and compares the residual.

use tree_sitter::{Node, Parser, Tree};

pub fn is_whitespace_or_comment_only(before: &[u8], after: &[u8]) -> bool {
    let parse = |s: &[u8]| -> Option<Tree> {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_rust::LANGUAGE.into()).ok()?;
        p.parse(s, None)
    };
    let Some(b_tree) = parse(before) else { return false };
    let Some(a_tree) = parse(after) else { return false };
    let mut b_toks: Vec<&[u8]> = Vec::new();
    let mut a_toks: Vec<&[u8]> = Vec::new();
    collect_significant_tokens(b_tree.root_node(), before, &mut b_toks);
    collect_significant_tokens(a_tree.root_node(), after, &mut a_toks);
    b_toks == a_toks
}

fn collect_significant_tokens<'a>(node: Node<'_>, src: &'a [u8], out: &mut Vec<&'a [u8]>) {
    let kind = node.kind();
    if kind == "line_comment" || kind == "block_comment" {
        return;
    }
    if node.child_count() == 0 {
        let s = &src[node.byte_range()];
        if !s.iter().all(|b| b.is_ascii_whitespace()) {
            out.push(s);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_significant_tokens(child, src, out);
    }
}
```

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): whitespace/comment-only diff detector"
```

---

### Task 12: Rename detection

**Files:**
- Modify: `benchmarks/provbench/labeler/src/diff/mod.rs` (add rename fns)

Per SPEC §5.2: when a symbol no longer resolves, search the post-commit tree for a candidate whose Myers-diff similarity over symbol-bearing lines is ≥0.6. Implementation: collect candidate names from the post-commit AST that share kind (fn/struct/etc.) with the missing symbol; compare line-content of the disappeared signature span vs. each candidate's signature span via `similar::TextDiff::ratio()`.

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::diff::rename_candidate;

#[test]
fn high_similarity_rename_is_detected() {
    let before = b"fn search_pattern(pat: &str) -> Vec<usize> { Vec::new() }";
    let after_candidates = vec![
        ("search_input".to_string(), b"fn search_input(pat: &str) -> Vec<usize> { Vec::new() }".to_vec()),
        ("totally_different".to_string(), b"fn totally_different() {}".to_vec()),
    ];
    let m = rename_candidate(before, &after_candidates, 0.6);
    assert_eq!(m.as_deref(), Some("search_input"));
}

#[test]
fn no_candidate_above_threshold_returns_none() {
    let before = b"fn alpha() {}";
    let after_candidates = vec![
        ("beta".to_string(), b"fn beta(x: u32, y: u32, z: u32) -> Vec<u32> { Vec::new() }".to_vec()),
    ];
    let m = rename_candidate(before, &after_candidates, 0.6);
    assert_eq!(m, None);
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
pub fn rename_candidate(
    before_span: &[u8],
    after_candidates: &[(String, Vec<u8>)],
    min_ratio: f32,
) -> Option<String> {
    let before = String::from_utf8_lossy(before_span);
    let mut best: Option<(String, f32)> = None;
    for (name, span) in after_candidates {
        let after = String::from_utf8_lossy(span);
        let ratio = similar::TextDiff::from_lines(&before, &after).ratio();
        if ratio >= min_ratio {
            match &best {
                None => best = Some((name.clone(), ratio)),
                Some((_, r)) if ratio > *r => best = Some((name.clone(), ratio)),
                _ => {}
            }
        }
    }
    best.map(|(n, _)| n)
}
```

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): symbol rename detection via similar"
```

---

### Task 13: Labeling rule engine

**Files:**
- Create: `benchmarks/provbench/labeler/src/label.rs`
- Modify: `benchmarks/provbench/labeler/src/lib.rs` (add `pub mod label;`)

Single function `classify(fact, post_commit_state) -> Label` applying SPEC §5 rules in order:
1. File missing → `stale_source_deleted`.
2. Symbol unresolved + rename detected → `stale_symbol_renamed`.
3. Symbol unresolved + no rename → `stale_source_deleted`.
4. Hash unchanged → `valid`.
5. Hash differs but whitespace/comment-only → `valid`.
6. Hash differs and structurally classifiable → `stale_source_changed`.
7. Otherwise → `needs_revalidation`.

`PostCommitState` is a small trait so unit tests can mock it without spinning up rust-analyzer.

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::ast::spans::Span;
use provbench_labeler::facts::Fact;
use provbench_labeler::label::{Label, classify, MockState};

fn fn_fact() -> Fact {
    Fact::FunctionSignature {
        qualified_name: "f".into(),
        source_path: "a.rs".into(),
        span: Span { byte_range: 0..16, line_start: 1, line_end: 1 },
        content_hash: "deadbeef".to_string(),
    }
}

#[test]
fn missing_file_yields_stale_source_deleted() {
    let st = MockState { file_exists: false, ..MockState::default() };
    assert_eq!(classify(&fn_fact(), &st), Label::StaleSourceDeleted);
}

#[test]
fn unresolved_symbol_with_rename_yields_renamed() {
    let st = MockState { file_exists: true, symbol_resolves: false,
        rename_candidate: Some("g".into()), ..MockState::default() };
    assert_eq!(classify(&fn_fact(), &st), Label::StaleSymbolRenamed { new_name: "g".into() });
}

#[test]
fn matching_hash_yields_valid() {
    let st = MockState { file_exists: true, symbol_resolves: true,
        post_span_hash: Some("deadbeef".into()), ..MockState::default() };
    assert_eq!(classify(&fn_fact(), &st), Label::Valid);
}

#[test]
fn whitespace_only_diff_yields_valid() {
    let st = MockState { file_exists: true, symbol_resolves: true,
        post_span_hash: Some("different".into()), whitespace_or_comment_only: true,
        ..MockState::default() };
    assert_eq!(classify(&fn_fact(), &st), Label::Valid);
}

#[test]
fn structural_change_yields_stale_source_changed() {
    let st = MockState { file_exists: true, symbol_resolves: true,
        post_span_hash: Some("different".into()),
        structurally_classifiable: true, ..MockState::default() };
    assert_eq!(classify(&fn_fact(), &st), Label::StaleSourceChanged);
}

#[test]
fn unclassifiable_change_yields_needs_revalidation() {
    let st = MockState { file_exists: true, symbol_resolves: true,
        post_span_hash: Some("different".into()), ..MockState::default() };
    assert_eq!(classify(&fn_fact(), &st), Label::NeedsRevalidation);
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
//! Mechanical labeling rule engine. SPEC §5 first-match-wins ordering.
//!
//! The rule order is the contract — never reorder without a §11 entry.

use crate::facts::Fact;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Label {
    Valid,
    StaleSourceChanged,
    StaleSourceDeleted,
    StaleSymbolRenamed { new_name: String },
    NeedsRevalidation,
}

pub trait PostCommitState {
    fn file_exists(&self) -> bool;
    fn symbol_resolves(&self) -> bool;
    fn rename_candidate(&self) -> Option<&str>;
    fn post_span_hash(&self) -> Option<&str>;
    fn whitespace_or_comment_only(&self) -> bool;
    fn structurally_classifiable(&self) -> bool;
}

pub fn classify(fact: &Fact, state: &dyn PostCommitState) -> Label {
    if !state.file_exists() {
        return Label::StaleSourceDeleted;
    }
    if !state.symbol_resolves() {
        return match state.rename_candidate() {
            Some(new_name) => Label::StaleSymbolRenamed { new_name: new_name.to_string() },
            None => Label::StaleSourceDeleted,
        };
    }
    let observed_hash = fact_hash(fact);
    if let Some(post) = state.post_span_hash() {
        if post == observed_hash {
            return Label::Valid;
        }
        if state.whitespace_or_comment_only() {
            return Label::Valid;
        }
        if state.structurally_classifiable() {
            return Label::StaleSourceChanged;
        }
        return Label::NeedsRevalidation;
    }
    Label::NeedsRevalidation
}

fn fact_hash(fact: &Fact) -> &str {
    match fact {
        Fact::FunctionSignature { content_hash, .. }
        | Fact::Field { content_hash, .. }
        | Fact::PublicSymbol { content_hash, .. } => content_hash,
        Fact::DocClaim { mention_hash, .. } => mention_hash,
        Fact::TestAssertion { content_hash, .. } => content_hash,
    }
}

#[derive(Debug, Default)]
pub struct MockState {
    pub file_exists: bool,
    pub symbol_resolves: bool,
    pub rename_candidate: Option<String>,
    pub post_span_hash: Option<String>,
    pub whitespace_or_comment_only: bool,
    pub structurally_classifiable: bool,
}

impl PostCommitState for MockState {
    fn file_exists(&self) -> bool { self.file_exists }
    fn symbol_resolves(&self) -> bool { self.symbol_resolves }
    fn rename_candidate(&self) -> Option<&str> { self.rename_candidate.as_deref() }
    fn post_span_hash(&self) -> Option<&str> { self.post_span_hash.as_deref() }
    fn whitespace_or_comment_only(&self) -> bool { self.whitespace_or_comment_only }
    fn structurally_classifiable(&self) -> bool { self.structurally_classifiable }
}
```

The `MockState` lives in the same module so tests don't need a separate fixtures crate but is `#[cfg(test)]`-gated only for `default()`/fields if clippy complains about dead code; otherwise leave public for crate-internal integration tests too.

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): labeling rule engine (SPEC §5 first-match-wins)"
```

---

### Task 14: Per-commit replay driver

**Files:**
- Create: `benchmarks/provbench/labeler/src/replay.rs`
- Modify: `benchmarks/provbench/labeler/src/lib.rs` (add `pub mod replay;`)

Glues everything: walks first-parent commits, at T₀ runs all extractors to build the **fact set**, then for each subsequent commit checks each fact's status against post-commit state (file, hash, symbol resolution, rename detection). Yields `(fact_id, commit_sha, Label)` rows.

The driver uses the file system at the commit's checkout (`git worktree add`/`gix` workspace per commit) — this is expensive; a future optimization is to read blobs directly without checkout, but for v1 correctness over speed.

Actually: rust-analyzer needs a real workspace on disk. For v1 we'll `git checkout` to a fresh worktree per commit. Use `gix` worktree API or shell out to `git worktree add` (preferred — gix worktree support is incomplete).

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::replay::{Replay, ReplayConfig};

#[test]
fn replay_over_synthetic_repo_emits_fact_at_commit_rows() {
    // Build a tiny 2-commit repo via shell; run replay; check counts.
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    let g = |args: &[&str]| {
        let s = std::process::Command::new("git").args(args).current_dir(p).status().unwrap();
        assert!(s.success(), "git {args:?} failed");
    };
    g(&["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").unwrap();
    std::fs::write(p.join("src/lib.rs"), b"pub fn ten() -> i32 { 10 }\n").unwrap();
    g(&["add", "."]);
    g(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "init"]);
    let t0 = String::from_utf8(std::process::Command::new("git")
        .args(["rev-parse", "HEAD"]).current_dir(p).output().unwrap().stdout).unwrap().trim().to_string();
    std::fs::write(p.join("src/lib.rs"), b"pub fn ten() -> i32 { 11 }\n").unwrap();
    g(&["add", "."]);
    g(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "tweak"]);
    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true, // unit-test path; rust-analyzer not required
    };
    let rows = Replay::run(&cfg).unwrap();
    // 1 fact (function signature) × 2 commits = 2 rows.
    assert_eq!(rows.len(), 2, "got {rows:?}");
    let labels: Vec<_> = rows.iter().map(|r| r.label.clone()).collect();
    assert!(labels.iter().any(|l| matches!(l, provbench_labeler::label::Label::Valid)));
    assert!(labels.iter().any(|l| matches!(l, provbench_labeler::label::Label::StaleSourceChanged)));
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
//! Per-commit replay: read blobs at each commit, compute post-commit
//! state per fact, classify, emit FactAtCommit rows.

use crate::ast::RustAst;
use crate::diff::{is_whitespace_or_comment_only, rename_candidate};
use crate::facts::{Fact, function_signature};
use crate::label::{Label, PostCommitState, classify};
use crate::repo::{CommitRef, Pilot};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactAtCommit {
    pub fact_id: String,    // "{kind}::{qualified}::{source}::{line_start}"
    pub commit_sha: String,
    pub label: Label,
}

pub struct ReplayConfig {
    pub repo_path: PathBuf,
    pub t0_sha: String,
    /// When true, the driver does not consult rust-analyzer; symbol
    /// resolution is approximated as "the bound source path still exists".
    /// Used by unit tests; production runs must set this false.
    pub skip_symbol_resolution: bool,
}

pub struct Replay;

impl Replay {
    pub fn run(cfg: &ReplayConfig) -> Result<Vec<FactAtCommit>> {
        let pilot = Pilot::open(&AdHocSpec {
            path: cfg.repo_path.clone(),
            t0_sha: cfg.t0_sha.clone(),
        })?;
        let commits: Vec<CommitRef> = pilot.walk_first_parent()?.collect();
        // Extract the fact set at T₀ across every .rs file at T₀.
        let mut facts: Vec<Fact> = Vec::new();
        for path in rust_paths_at(&pilot, &cfg.t0_sha)? {
            if let Some(blob) = pilot.read_blob_at(&cfg.t0_sha, &path)? {
                let ast = RustAst::parse(&blob)?;
                facts.extend(function_signature::extract(&ast, &path));
            }
        }
        let mut rows = Vec::new();
        for commit in &commits {
            for fact in &facts {
                let label = classify_against_commit(&pilot, fact, &commit.sha, cfg)?;
                rows.push(FactAtCommit {
                    fact_id: fact_id(fact),
                    commit_sha: commit.sha.clone(),
                    label,
                });
            }
        }
        Ok(rows)
    }
}

struct AdHocSpec {
    path: PathBuf,
    t0_sha: String,
}

impl crate::repo::PilotRepoSpec for AdHocSpec {
    fn local_clone_path(&self) -> &Path { &self.path }
    fn t0_sha(&self) -> &str { &self.t0_sha }
}

fn rust_paths_at(pilot: &Pilot, sha: &str) -> Result<Vec<PathBuf>> {
    // Use git ls-tree -r --name-only <sha> filtered by .rs.
    let out = std::process::Command::new("git")
        .args(["-C"]).arg(pilot_repo_path(pilot)).args(["ls-tree", "-r", "--name-only", sha])
        .output()
        .with_context(|| format!("ls-tree {sha}"))?;
    if !out.status.success() {
        anyhow::bail!("git ls-tree failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.ends_with(".rs"))
        .map(PathBuf::from)
        .collect())
}

fn pilot_repo_path(_p: &Pilot) -> &Path {
    // Pilot stores its gix::Repository; expose its working dir via a
    // small accessor added to repo.rs.
    unimplemented!("expose Pilot::repo_path() in Task 14");
}

fn fact_id(fact: &Fact) -> String {
    match fact {
        Fact::FunctionSignature { qualified_name, source_path, span, .. } => {
            format!("FunctionSignature::{qualified_name}::{}::{}",
                source_path.display(), span.line_start)
        }
        Fact::Field { qualified_path, source_path, span, .. } => {
            format!("Field::{qualified_path}::{}::{}",
                source_path.display(), span.line_start)
        }
        Fact::PublicSymbol { qualified_name, source_path, span, .. } => {
            format!("PublicSymbol::{qualified_name}::{}::{}",
                source_path.display(), span.line_start)
        }
        Fact::DocClaim { qualified_name, doc_path, mention_span, .. } => {
            format!("DocClaim::{qualified_name}::{}::{}",
                doc_path.display(), mention_span.line_start)
        }
        Fact::TestAssertion { test_fn, source_path, span, .. } => {
            format!("TestAssertion::{test_fn}::{}::{}",
                source_path.display(), span.line_start)
        }
    }
}

struct CommitState<'a> {
    file_exists: bool,
    post_span_hash: Option<String>,
    structurally_classifiable: bool,
    whitespace_or_comment_only: bool,
    symbol_resolves: bool,
    rename: Option<String>,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> PostCommitState for CommitState<'a> {
    fn file_exists(&self) -> bool { self.file_exists }
    fn symbol_resolves(&self) -> bool { self.symbol_resolves }
    fn rename_candidate(&self) -> Option<&str> { self.rename.as_deref() }
    fn post_span_hash(&self) -> Option<&str> { self.post_span_hash.as_deref() }
    fn whitespace_or_comment_only(&self) -> bool { self.whitespace_or_comment_only }
    fn structurally_classifiable(&self) -> bool { self.structurally_classifiable }
}

fn classify_against_commit(
    pilot: &Pilot,
    fact: &Fact,
    commit_sha: &str,
    cfg: &ReplayConfig,
) -> Result<Label> {
    let path = match fact {
        Fact::FunctionSignature { source_path, .. }
        | Fact::Field { source_path, .. }
        | Fact::PublicSymbol { source_path, .. }
        | Fact::TestAssertion { source_path, .. } => source_path.clone(),
        Fact::DocClaim { doc_path, .. } => doc_path.clone(),
    };
    let blob = pilot.read_blob_at(commit_sha, &path)?;
    let state = match blob {
        None => CommitState {
            file_exists: false, post_span_hash: None, structurally_classifiable: false,
            whitespace_or_comment_only: false, symbol_resolves: false, rename: None,
            _phantom: Default::default(),
        },
        Some(post) => {
            let observed_hash = match fact {
                Fact::FunctionSignature { content_hash, .. } => content_hash.clone(),
                Fact::Field { content_hash, .. } => content_hash.clone(),
                Fact::PublicSymbol { content_hash, .. } => content_hash.clone(),
                Fact::DocClaim { mention_hash, .. } => mention_hash.clone(),
                Fact::TestAssertion { content_hash, .. } => content_hash.clone(),
            };
            let qualified_name = match fact {
                Fact::FunctionSignature { qualified_name, .. } => qualified_name.clone(),
                Fact::Field { qualified_path, .. } => qualified_path.clone(),
                Fact::PublicSymbol { qualified_name, .. } => qualified_name.clone(),
                Fact::DocClaim { qualified_name, .. } => qualified_name.clone(),
                Fact::TestAssertion { test_fn, .. } => test_fn.clone(),
            };
            let post_ast = RustAst::parse(&post).ok();
            let post_signature = post_ast.as_ref().and_then(|a| {
                function_signature::extract(a, &path)
                    .find_map(|f| match f {
                        Fact::FunctionSignature { qualified_name: q, span, content_hash, .. }
                            if q == qualified_name => Some((span, content_hash, post.clone())),
                        _ => None,
                    })
            });
            let (post_hash, ws_only, structural, symbol_resolves, rename) = match post_signature {
                Some((post_span, post_hash, _post_bytes)) => {
                    let pre_span = match fact {
                        Fact::FunctionSignature { span, .. } => span.clone(),
                        _ => post_span.clone(),
                    };
                    let before_bytes = blob_at_t0_for_span(pilot, &cfg.t0_sha, &path, &pre_span)?;
                    let after_bytes = post[post_span.byte_range.clone()].to_vec();
                    let ws = is_whitespace_or_comment_only(&before_bytes, &after_bytes);
                    let structural = post_hash != observed_hash; // any structural delta counts
                    (Some(post_hash), ws, structural, true, None)
                }
                None => {
                    if cfg.skip_symbol_resolution {
                        (None, false, false, false, None)
                    } else {
                        let sigs: Vec<(String, Vec<u8>)> = post_ast
                            .iter()
                            .flat_map(|a| {
                                function_signature::extract(a, &path).filter_map(|f| match f {
                                    Fact::FunctionSignature { qualified_name, span, .. } => {
                                        let bytes = post[span.byte_range].to_vec();
                                        Some((qualified_name, bytes))
                                    }
                                    _ => None,
                                })
                            })
                            .collect();
                        let pre_span = match fact {
                            Fact::FunctionSignature { span, .. } => span.clone(),
                            _ => return Ok(Label::NeedsRevalidation),
                        };
                        let before_bytes = blob_at_t0_for_span(pilot, &cfg.t0_sha, &path, &pre_span)?;
                        let rename = rename_candidate(&before_bytes, &sigs, 0.6);
                        (None, false, false, false, rename)
                    }
                }
            };
            CommitState {
                file_exists: true,
                post_span_hash: post_hash,
                structurally_classifiable: structural,
                whitespace_or_comment_only: ws_only,
                symbol_resolves,
                rename,
                _phantom: Default::default(),
            }
        }
    };
    Ok(classify(fact, &state))
}

fn blob_at_t0_for_span(pilot: &Pilot, t0: &str, path: &Path, span: &crate::ast::spans::Span) -> Result<Vec<u8>> {
    let blob = pilot.read_blob_at(t0, path)?
        .with_context(|| format!("t0 blob {} missing", path.display()))?;
    Ok(blob[span.byte_range.clone()].to_vec())
}
```

This file is ~250 lines and represents the bulk of integration work. Add a small `Pilot::repo_path() -> &Path` accessor in `repo.rs` to support `rust_paths_at`.

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

The synthetic-repo test runs without rust-analyzer (because of `skip_symbol_resolution`).

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): per-commit replay driver"
```

---

### Task 15: JSONL output + labeler-SHA stamping

**Files:**
- Create: `benchmarks/provbench/labeler/src/output.rs`
- Modify: `benchmarks/provbench/labeler/src/lib.rs` (add `pub mod output;`)
- Modify: `benchmarks/provbench/labeler/src/main.rs` (add `run` subcommand)

Output format: one JSON object per line, sorted deterministically by `(fact_id, commit_sha)`. Each row carries the labeler git SHA stamp (read from `PROVBENCH_LABELER_GIT_SHA` build-time env or computed at runtime via `git -C <bin>` — runtime is more reliable for incremental dev). Output goes to `benchmarks/provbench/corpus/<repo>-<t0_short>-<labeler_sha_short>.jsonl`.

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::output::{write_jsonl, OutputRow};
use provbench_labeler::label::Label;

#[test]
fn rows_serialize_sorted_with_labeler_stamp() {
    let rows = vec![
        OutputRow { fact_id: "B".into(), commit_sha: "c1".into(), label: Label::Valid },
        OutputRow { fact_id: "A".into(), commit_sha: "c2".into(), label: Label::Valid },
        OutputRow { fact_id: "A".into(), commit_sha: "c1".into(), label: Label::StaleSourceChanged },
    ];
    let tmp = tempfile::NamedTempFile::new().unwrap();
    write_jsonl(tmp.path(), &rows, "labelersha123").unwrap();
    let body = std::fs::read_to_string(tmp.path()).unwrap();
    let lines: Vec<_> = body.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains(r#""fact_id":"A""#) && lines[0].contains(r#""commit_sha":"c1""#));
    assert!(lines[1].contains(r#""fact_id":"A""#) && lines[1].contains(r#""commit_sha":"c2""#));
    assert!(lines[2].contains(r#""fact_id":"B""#));
    for line in lines {
        assert!(line.contains(r#""labeler_git_sha":"labelersha123""#));
    }
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
//! Deterministic JSONL output with labeler-SHA stamping.
//!
//! Determinism contract: byte-identical output across runs given the
//! same labeler git SHA, repo, and T₀.

use crate::label::Label;
use anyhow::Result;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct OutputRow {
    pub fact_id: String,
    pub commit_sha: String,
    pub label: Label,
}

#[derive(Debug, Serialize)]
struct Stamped<'a> {
    fact_id: &'a str,
    commit_sha: &'a str,
    label: &'a Label,
    labeler_git_sha: &'a str,
}

pub fn write_jsonl(path: &Path, rows: &[OutputRow], labeler_git_sha: &str) -> Result<()> {
    let mut sorted: Vec<&OutputRow> = rows.iter().collect();
    sorted.sort_by(|a, b| a.fact_id.cmp(&b.fact_id).then_with(|| a.commit_sha.cmp(&b.commit_sha)));
    let mut f = std::fs::File::create(path)?;
    for row in sorted {
        let stamped = Stamped {
            fact_id: &row.fact_id,
            commit_sha: &row.commit_sha,
            label: &row.label,
            labeler_git_sha,
        };
        serde_json::to_writer(&mut f, &stamped)?;
        f.write_all(b"\n")?;
    }
    f.flush()?;
    Ok(())
}

pub fn current_labeler_sha() -> Result<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;
    if !out.status.success() {
        anyhow::bail!("git rev-parse HEAD failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}
```

Wire CLI: `provbench-labeler run --repo <path> --t0 <sha> --out <path>` invokes `Replay::run` then `write_jsonl`.

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): deterministic JSONL output with labeler-SHA stamping"
```

---

### Task 16: Determinism harness

**Files:**
- Create: `benchmarks/provbench/labeler/tests/determinism.rs`

End-to-end test that runs `Replay::run` twice on a synthetic repo and asserts byte-identical JSONL output.

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::output::{write_jsonl, OutputRow};
use provbench_labeler::replay::{Replay, ReplayConfig};

#[test]
fn two_runs_produce_byte_identical_output() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    let g = |args: &[&str]| {
        let s = std::process::Command::new("git").args(args).current_dir(p).status().unwrap();
        assert!(s.success(), "git {args:?}");
    };
    g(&["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").unwrap();
    std::fs::write(p.join("src/lib.rs"), b"pub fn ten() -> i32 { 10 }\n").unwrap();
    g(&["add", "."]);
    g(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "init"]);
    let t0 = String::from_utf8(std::process::Command::new("git")
        .args(["rev-parse", "HEAD"]).current_dir(p).output().unwrap().stdout).unwrap().trim().to_string();
    std::fs::write(p.join("src/lib.rs"), b"pub fn ten() -> i32 { 11 }\n").unwrap();
    g(&["add", "."]);
    g(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "tweak"]);

    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(), t0_sha: t0.clone(), skip_symbol_resolution: true,
    };
    let rows1 = Replay::run(&cfg).unwrap();
    let rows2 = Replay::run(&cfg).unwrap();
    let out1 = tempfile::NamedTempFile::new().unwrap();
    let out2 = tempfile::NamedTempFile::new().unwrap();
    let to_output = |rs: Vec<provbench_labeler::replay::FactAtCommit>| -> Vec<OutputRow> {
        rs.into_iter().map(|r| OutputRow {
            fact_id: r.fact_id, commit_sha: r.commit_sha, label: r.label,
        }).collect()
    };
    write_jsonl(out1.path(), &to_output(rows1), "stamp").unwrap();
    write_jsonl(out2.path(), &to_output(rows2), "stamp").unwrap();
    let b1 = std::fs::read(out1.path()).unwrap();
    let b2 = std::fs::read(out2.path()).unwrap();
    assert_eq!(b1, b2, "labeler is non-deterministic");
}
```

- [ ] **Step 2: Run test, verify it passes** (all dependencies are already implemented)

Expected: PASS.

- [ ] **Step 3: Verify fmt + clippy**

- [ ] **Step 4: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "test(provbench): determinism harness over synthetic repo"
```

---

### Task 17: Spot-check sampler

**Files:**
- Create: `benchmarks/provbench/labeler/src/spotcheck.rs`
- Modify: `benchmarks/provbench/labeler/src/lib.rs` (add `pub mod spotcheck;`)
- Modify: `benchmarks/provbench/labeler/src/main.rs` (add `spotcheck` subcommand)

Sample 200 rows uniformly at random from the corpus using a fixed seed (`0xC0DEBABE_DEADBEEF`). Stratification: bucket by ground-truth label and sample proportionally to ensure rare classes are represented; minimum floor of 20 per non-empty class. Output a CSV with columns `(fact_id, commit_sha, fact_kind, predicted_label, source_path, span_lines, ground_truth_label_blank)` to `benchmarks/provbench/spotcheck/sample-<labeler_sha_short>.csv`.

The "ground_truth_label_blank" column is empty so the human reviewer fills it; the predicted label is left visible — the SPEC §9.1 process is "agreement", not "blind grading".

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::spotcheck::{sample, Sampled};
use provbench_labeler::output::OutputRow;
use provbench_labeler::label::Label;

#[test]
fn deterministic_sampler_returns_same_indices_across_runs() {
    let rows: Vec<OutputRow> = (0..1000).map(|i| OutputRow {
        fact_id: format!("f{i}"),
        commit_sha: format!("c{}", i % 10),
        label: if i % 5 == 0 { Label::StaleSourceChanged } else { Label::Valid },
    }).collect();
    let s1 = sample(&rows, 200);
    let s2 = sample(&rows, 200);
    assert_eq!(s1.len(), 200);
    assert_eq!(s1, s2);
}

#[test]
fn rare_classes_meet_min_floor() {
    let rows: Vec<OutputRow> = (0..1000).map(|i| OutputRow {
        fact_id: format!("f{i}"),
        commit_sha: format!("c{i}"),
        label: match i % 100 {
            0..=1 => Label::StaleSymbolRenamed { new_name: "x".into() },
            2..=3 => Label::StaleSourceDeleted,
            _ => Label::Valid,
        },
    }).collect();
    let s = sample(&rows, 200);
    let renamed = s.iter().filter(|r| matches!(r.row.label, Label::StaleSymbolRenamed { .. })).count();
    assert!(renamed >= 10, "rare class under-sampled: got {renamed}");
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
//! Stratified deterministic sampler for the spot-check process.
//! Seed is fixed (`0xC0DEBABE_DEADBEEF`) so re-running produces the same
//! CSV — important when the human reviewer fills it in over multiple
//! sessions.

use crate::output::OutputRow;
use rand::SeedableRng;
use rand::seq::SliceRandom;

const SEED: u64 = 0xC0DE_BABE_DEAD_BEEF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sampled {
    pub row: OutputRow,
    pub bucket: String,
}

pub fn sample(rows: &[OutputRow], n: usize) -> Vec<Sampled> {
    use std::collections::BTreeMap;
    let mut buckets: BTreeMap<String, Vec<&OutputRow>> = BTreeMap::new();
    for r in rows {
        buckets.entry(label_bucket(&r.label)).or_default().push(r);
    }
    let total = rows.len();
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(SEED);
    let mut out = Vec::new();
    let class_count = buckets.len().max(1);
    let per_class_floor = (n / (class_count * 2)).max(10).min(n);
    let mut deficit_pool: Vec<Sampled> = Vec::new();
    for (label, mut items) in buckets {
        items.shuffle(&mut rng);
        let proportional = ((items.len() as f64 / total as f64) * n as f64).round() as usize;
        let take = proportional.max(per_class_floor).min(items.len());
        for r in items.iter().take(take) {
            out.push(Sampled { row: (*r).clone(), bucket: label.clone() });
        }
        for r in items.iter().skip(take) {
            deficit_pool.push(Sampled { row: (*r).clone(), bucket: label.clone() });
        }
    }
    if out.len() > n {
        out.truncate(n);
    } else if out.len() < n {
        deficit_pool.shuffle(&mut rng);
        for s in deficit_pool.into_iter().take(n - out.len()) {
            out.push(s);
        }
    }
    out.sort_by(|a, b| a.row.fact_id.cmp(&b.row.fact_id).then_with(|| a.row.commit_sha.cmp(&b.row.commit_sha)));
    out
}

fn label_bucket(label: &crate::label::Label) -> String {
    use crate::label::Label::*;
    match label {
        Valid => "valid".into(),
        StaleSourceChanged => "stale_source_changed".into(),
        StaleSourceDeleted => "stale_source_deleted".into(),
        StaleSymbolRenamed { .. } => "stale_symbol_renamed".into(),
        NeedsRevalidation => "needs_revalidation".into(),
    }
}

pub fn write_csv(path: &std::path::Path, samples: &[Sampled]) -> anyhow::Result<()> {
    let mut f = std::fs::File::create(path)?;
    use std::io::Write;
    writeln!(f, "fact_id,commit_sha,bucket,predicted_label,human_label,disagreement_notes")?;
    for s in samples {
        let predicted = label_bucket(&s.row.label);
        writeln!(f, "{},{},{},{},,",
            csv_escape(&s.row.fact_id),
            csv_escape(&s.row.commit_sha),
            csv_escape(&s.bucket),
            csv_escape(&predicted),
        )?;
    }
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
```

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): deterministic stratified spot-check sampler"
```

---

### Task 18: Wilson 95% CI report

**Files:**
- Modify: `benchmarks/provbench/labeler/src/spotcheck.rs` (add `wilson_lower_bound` + `report` fns)
- Modify: `benchmarks/provbench/labeler/src/main.rs` (add `report` subcommand)

Compute the Wilson lower bound at 95% confidence given `(agree_count, total)`. Use the closed-form formula — no external crate. SPEC §9.1: gate is met iff point estimate ≥95%; report both.

- [ ] **Step 1: Write failing test**

```rust
use provbench_labeler::spotcheck::wilson_lower_bound_95;

#[test]
fn wilson_lower_bound_at_perfect_score() {
    let lb = wilson_lower_bound_95(200, 200);
    assert!(lb > 0.98, "got {lb}");
}

#[test]
fn wilson_lower_bound_at_95_point_estimate() {
    let lb = wilson_lower_bound_95(190, 200);
    // analytic: ~0.910
    assert!(lb > 0.90 && lb < 0.93, "got {lb}");
}

#[test]
fn wilson_lower_bound_zero_total_returns_zero() {
    assert_eq!(wilson_lower_bound_95(0, 0), 0.0);
}
```

- [ ] **Step 2: Run failing test**

Expected: FAIL — function doesn't exist.

- [ ] **Step 3: Implement**

```rust
/// Wilson score lower bound at 95% confidence (z=1.95996398454).
pub fn wilson_lower_bound_95(success: u32, total: u32) -> f64 {
    if total == 0 { return 0.0; }
    let n = total as f64;
    let p = success as f64 / n;
    let z: f64 = 1.95996398454;
    let denom = 1.0 + (z * z) / n;
    let center = p + (z * z) / (2.0 * n);
    let margin = z * ((p * (1.0 - p) + (z * z) / (4.0 * n)) / n).sqrt();
    (center - margin) / denom
}

#[derive(Debug, Clone)]
pub struct SpotCheckReport {
    pub total: u32,
    pub agree: u32,
    pub point_estimate: f64,
    pub wilson_lower_95: f64,
    pub gate_passed: bool,
}

pub fn report(agree: u32, total: u32) -> SpotCheckReport {
    let p = if total == 0 { 0.0 } else { agree as f64 / total as f64 };
    let wlb = wilson_lower_bound_95(agree, total);
    SpotCheckReport {
        total,
        agree,
        point_estimate: p,
        wilson_lower_95: wlb,
        gate_passed: p >= 0.95 && total >= 200,
    }
}
```

CLI: `provbench-labeler report --csv <filled-csv>` reads the human_label column, computes agree/total, prints the report as both human-readable and JSON. Append the report to `benchmarks/provbench/spotcheck/report-<labeler_sha_short>.md`.

- [ ] **Step 4: Verify all tests pass + fmt + clippy**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/provbench/labeler/
git commit -m "feat(provbench): Wilson 95% CI spot-check report"
```

---

### Task 19: Reproducibility documentation

**Files:**
- Create: `benchmarks/provbench/labeler/README.md`
- Create: `benchmarks/provbench/corpus/.gitkeep`
- Create: `benchmarks/provbench/spotcheck/.gitkeep`

The README is the artifact a third party uses to reproduce the corpus. It must answer: how to clone the pilot, how to verify tooling, how to run the labeler, where the output lands, how to spot-check, how to re-run for determinism.

- [ ] **Step 1: Write the README**

```markdown
# ProvBench Phase 0b Labeler

Mechanical labeler for the ProvBench-CodeContext pilot corpus.
**Frozen contract:** `../SPEC.md`. This crate is excluded from the
ironrace-memory workspace because Phase 0 must be releasable as a
standalone reproducible artifact.

## Reproducing the pilot corpus

1. **Verify tooling pins.**
   ```
   cargo run -p provbench-labeler -- verify-tooling
   ```
   The expected hashes are SPEC §13.1. A mismatch is fatal; do not
   attempt to "work around" it.

2. **Clone the pilot at T₀.**
   ```
   mkdir -p benchmarks/provbench/work
   git clone https://github.com/BurntSushi/ripgrep \
     benchmarks/provbench/work/ripgrep
   git -C benchmarks/provbench/work/ripgrep checkout \
     af6b6c543b224d348a8876f0c06245d9ea7929c5
   ```

3. **Run the labeler.**
   ```
   cargo run --release -p provbench-labeler -- run \
     --repo benchmarks/provbench/work/ripgrep \
     --t0 af6b6c543b224d348a8876f0c06245d9ea7929c5 \
     --out benchmarks/provbench/corpus/ripgrep-af6b6c54-$(git rev-parse --short HEAD).jsonl
   ```
   Output is JSONL, one `fact_at_commit` row per line, sorted
   `(fact_id, commit_sha)`. Every row carries `labeler_git_sha` matching
   the labeler commit at run time.

4. **Determinism check.**
   ```
   cargo test --release -p provbench-labeler --test determinism
   ```
   And on the real corpus:
   ```
   <re-run step 3 with --out file2>
   diff <file1> <file2>
   ```
   The diff must be empty.

5. **Spot-check sampling.**
   ```
   cargo run -p provbench-labeler -- spotcheck \
     --corpus benchmarks/provbench/corpus/<file>.jsonl \
     --out benchmarks/provbench/spotcheck/sample-$(git rev-parse --short HEAD).csv
   ```
   Open the CSV and fill the `human_label` column for each of the 200
   rows. Save and run:
   ```
   cargo run -p provbench-labeler -- report \
     --csv benchmarks/provbench/spotcheck/sample-<sha>.csv
   ```
   The report prints the point estimate and Wilson 95% lower bound, and
   appends a Markdown summary under `benchmarks/provbench/spotcheck/`.

## Acceptance gate (SPEC §9.1)

Phase 0b is accepted iff:
- The point estimate of spot-check agreement is **≥95%**.
- The Wilson 95% lower bound is reported (informational; not part of
  the gate but required by §9.1).
- The determinism check is byte-identical.

## Limitations

- v1 supports Rust only. The held-out Python repo (`flask`) is **not**
  exercised by this labeler. A `tree-sitter`-based Python path is
  future work and **not** required for Phase 0b acceptance.
- `rust-analyzer` is invoked over LSP stdio per commit. Wall-clock cost
  scales with commit count; the pilot run on ripgrep at T₀ → T₀+~600
  commits is expected to take 30–90 minutes on an M-series Mac.
```

- [ ] **Step 2: Create the corpus + spotcheck placeholders**

```bash
touch benchmarks/provbench/corpus/.gitkeep
touch benchmarks/provbench/spotcheck/.gitkeep
```

- [ ] **Step 3: Verify**

```bash
cargo fmt --manifest-path benchmarks/provbench/labeler/Cargo.toml --all -- --check
cargo clippy --manifest-path benchmarks/provbench/labeler/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path benchmarks/provbench/labeler/Cargo.toml
```

All green.

- [ ] **Step 4: Commit**

```bash
git add benchmarks/provbench/
git commit -m "docs(provbench): phase 0b labeler reproducibility README"
```

---

## Self-Review

**Spec coverage check (SPEC.md → tasks):**
- §3.1 fact types: function signature (T5), field (T6), public symbol (T7), doc claim (T8), test assertion (T9). ✓
- §3.2 exclusions: not implemented (correct — they are excluded). ✓
- §4 mutation labels: `valid`, `stale_source_changed`, `stale_source_deleted`, `stale_symbol_renamed`, `needs_revalidation` all in `Label` enum (T13). ✓
- §5 labeling rules: `classify` in T13 implements the first-match-wins order with delegated state. Whitespace/comment-only path = T11; rename detection = T12. ✓
- §6 LLM baseline: out of scope for Phase 0b. ✓
- §9.1 acceptance gate: spot-check sampler (T17), Wilson CI report (T18), determinism harness (T16). ✓
- §10 anti-leakage: pilot/held-out separation enforced by `RIPGREP_T0_SHA` constant (T3); held-out repos are not touched by this plan. ✓
- §13.1 tooling pins: `verify_binary_hash` against the exact SHA-256s (T2). ✓
- §13.2 pilot repo pin: `RIPGREP_T0_SHA` + URL (T3). Held-out repos referenced in README only. ✓

**Placeholder scan:** None. Every task has full code blocks for new files; modifications are scoped with surrounding context. The "ground_truth_label_blank" CSV column in T17 is intentional — that's the human reviewer's fill-in.

**Type consistency:** `Fact` enum variants are introduced in T5 (`FunctionSignature`), T6 (`Field`), T7 (`PublicSymbol`), T8 (`DocClaim`), T9 (`TestAssertion`); T13 `fact_hash` and `replay::fact_id` reference them by exact field names. `Span` is defined in T4 and referenced in every fact variant. `Label` is defined in T13 and used unchanged in T15/T17/T18. `PostCommitState` trait + `MockState` impl + `CommitState` impl agree on method signatures.

**One known follow-up (not a plan defect):** the `tree_sitter::Node::utf8_text` API may return an error type that needs explicit handling depending on the tree-sitter crate minor version. If a subagent hits this, switch to slicing `&src[node.byte_range()]` and `std::str::from_utf8`. This is a 5-line local fix and does not affect the plan's structure.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-09-provbench-phase-0b-labeler.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

**Which approach?**
