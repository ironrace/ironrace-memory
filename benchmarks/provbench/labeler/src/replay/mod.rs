//! Per-commit replay: read blobs at each commit, compute post-commit state
//! per fact, classify, emit `FactAtCommit` rows.
//!
//! [`Replay::run`] is the production entry point: it extracts the T₀ fact
//! set across every `.rs` and eligible `.md` file in the pilot tree,
//! walks the first-parent commit chain, and applies the SPEC §5 rule
//! engine ([`crate::label::classify`]) to each (fact, commit) pair.
//! Output is unsorted; deterministic ordering is the writer's
//! responsibility (see [`crate::output::write_jsonl`]).
//!
//! ## Symbol resolution model (Task 3 / Cluster B)
//! Classification is commit-tree-local.  For each commit a
//! [`commit_index::CommitSymbolIndex`] is built from that commit's blobs
//! before any fact is classified; `rust-analyzer` is **not** consulted
//! at replay time.  Live RA tooling stays in the crate for
//! `tests/replay_ra.rs` (pinned-binary test) and for future
//! cross-crate / macro-expanded work.

pub mod commit_index;
mod match_post;

use crate::ast::{spans::Span, RustAst};
use crate::diff::{is_whitespace_or_comment_only, rename_candidate_typed, RenameOrigin};
use crate::facts::{doc_claim, field, function_signature, symbol_existence, test_assertion, Fact};
use crate::label::{classify, Label, PostCommitState};
use crate::repo::{normalize_path_for_fact_id, CommitRef, Pilot, PilotRepoSpec};
use crate::resolve::SymbolResolver;
use anyhow::{Context, Result};
use commit_index::CommitSymbolIndex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

// ── Public surface ────────────────────────────────────────────────────────────

/// One row of the SPEC §3 `fact_at_commit` corpus: a fact ID paired with
/// the label it carries at a specific first-parent descendant of T₀.
///
/// Stamped with the labeler git SHA at write time by
/// [`crate::output::write_jsonl`]; the in-memory form here is unstamped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactAtCommit {
    /// `"{kind}::{qualified}::{source}::{line_start}"`
    pub fact_id: String,
    pub commit_sha: String,
    pub label: Label,
}

/// Inputs to [`Replay::run`].
pub struct ReplayConfig {
    pub repo_path: PathBuf,
    pub t0_sha: String,
    /// When `true`, the per-commit symbol index is not built and cross-file
    /// symbol lookup is skipped entirely.  Symbol resolution is then
    /// approximated as "the fact's qualified name still appears at its
    /// original source path".  Used by unit tests; production runs must set
    /// this `false`.
    pub skip_symbol_resolution: bool,
}

/// Stateless entry point — see [`Replay::run`].
pub struct Replay;

struct ObservedFact {
    fact: Fact,
    t0_span_bytes: Vec<u8>,
    /// Zero-based position of this `Fact::TestAssertion` among same-
    /// `(source_path, test_fn)` siblings, in tree-sitter extraction
    /// order. `None` for every other fact kind. Used by
    /// `match_post::matching_post_fact` to pair T₀ assertion #N with
    /// post-commit assertion #N inside the same test fn — see SPEC §5
    /// rationale in
    /// `benchmarks/provbench/spotcheck/2026-05-12-post-pass3-findings.md`.
    test_assertion_ordinal: Option<usize>,
}

impl Replay {
    /// Run the full T₀-anchored replay and return one [`FactAtCommit`] row
    /// per (fact, commit) pair.
    ///
    /// Steps:
    /// 1. Extract the closed enum of facts from every `.rs` file present at
    ///    `cfg.t0_sha`, then doc-claim facts from eligible markdown files.
    /// 2. Walk first-parent descendants of T₀ and classify every fact
    ///    against each commit's blobs via the SPEC §5 rule engine.
    ///    Classification is commit-tree-local: a [`CommitSymbolIndex`] is
    ///    built from each commit's blobs before any fact is classified.
    ///    `rust-analyzer` is **not** consulted at replay time.
    ///
    /// Fail-closed: returns `Err` on invalid UTF-8 in a markdown blob
    /// (silently degrading would corrupt the corpus).
    pub fn run(cfg: &ReplayConfig) -> Result<Vec<FactAtCommit>> {
        Self::run_inner(cfg, None)
    }

    /// Test-only entry point preserved for API compatibility after Task 3.
    /// The `resolver` parameter is accepted but never invoked; per-commit
    /// classification uses [`CommitSymbolIndex`] exclusively.
    /// Use [`Self::run`] for production.
    #[doc(hidden)]
    pub fn run_with_resolver(
        cfg: &ReplayConfig,
        _resolver: Option<Box<dyn SymbolResolver>>,
    ) -> Result<Vec<FactAtCommit>> {
        // skip_symbol_resolution must be false so the CommitSymbolIndex is
        // built and per-commit tree resolution is exercised.  Returns Err
        // rather than panicking — this is a `pub` entry point even with
        // `#[doc(hidden)]`.
        anyhow::ensure!(
            !cfg.skip_symbol_resolution,
            "run_with_resolver: set skip_symbol_resolution=false so the \
             per-commit tree resolution path is exercised"
        );
        Self::run_inner(cfg, None)
    }

    /// Shared implementation used by both [`Self::run`] and
    /// [`Self::run_with_resolver`].
    ///
    /// When `cfg.skip_symbol_resolution` is `false`, a
    /// [`CommitSymbolIndex`] is built for each commit (reusing blobs already
    /// read for fact-source paths) and used as the authoritative source for
    /// cross-file symbol presence checks.
    fn run_inner(
        cfg: &ReplayConfig,
        _resolver: Option<Box<dyn SymbolResolver>>,
    ) -> Result<Vec<FactAtCommit>> {
        let pilot = Pilot::open(&AdHocSpec {
            path: cfg.repo_path.clone(),
            t0_sha: cfg.t0_sha.clone(),
        })?;
        let commits: Vec<CommitRef> = pilot.walk_first_parent()?.collect();

        // Extract the fact set at T₀ across every .rs file present at T₀.
        //
        // We also stash each fact-source blob's T₀ bytes in `t0_blobs` so
        // the per-commit step-3 fast-path below can compare them to the
        // post-commit bytes without re-reading. The map is bounded by the
        // number of fact-source paths and is tiny relative to the corpus;
        // see SPEC §5 invariant in
        // `benchmarks/provbench/spotcheck/2026-05-12-post-pass3-findings.md`.
        let mut facts: Vec<ObservedFact> = Vec::new();
        let mut t0_blobs: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
        let rust_paths = rust_paths_at(&pilot, &cfg.t0_sha)?;
        let mut facts_so_far: Vec<Fact> = Vec::new();
        for path in &rust_paths {
            if let Some(blob) = pilot.read_blob_at(&cfg.t0_sha, path)? {
                let ast = RustAst::parse(&blob)
                    .with_context(|| format!("parse {} @ T0", path.display()))?;
                push_observed_facts(
                    &mut facts,
                    &mut facts_so_far,
                    &blob,
                    function_signature::extract(&ast, path),
                );
                push_observed_facts(
                    &mut facts,
                    &mut facts_so_far,
                    &blob,
                    field::extract(&ast, path),
                );
                push_observed_facts(
                    &mut facts,
                    &mut facts_so_far,
                    &blob,
                    symbol_existence::extract(&ast, path),
                );
                let test_facts: Vec<Fact> =
                    test_assertion::extract(&ast, path, &facts_so_far).collect();
                push_test_assertion_facts(&mut facts, &mut facts_so_far, &blob, path, test_facts);
                t0_blobs.insert(path.clone(), blob);
            }
        }
        let rust_dirs = rust_paths
            .iter()
            .filter_map(|p| p.parent().map(Path::to_path_buf))
            .collect::<std::collections::BTreeSet<_>>();
        for path in markdown_paths_at(&pilot, &cfg.t0_sha)?
            .into_iter()
            .filter(|p| is_replay_doc_path(p, &rust_dirs))
        {
            if let Some(blob) = pilot.read_blob_at(&cfg.t0_sha, &path)? {
                let doc_facts =
                    doc_claim::extract(&blob, &path, &facts_so_far).with_context(|| {
                        format!("parse README at {} @ {}", path.display(), cfg.t0_sha)
                    })?;
                push_observed_facts(&mut facts, &mut facts_so_far, &blob, doc_facts);
                t0_blobs.insert(path, blob);
            }
        }

        #[cfg(test)]
        crate::repo::reset_read_blob_at_call_count();

        let facts_by_path = group_facts_by_source_path(&facts);
        // Build the T₀ qualified-name index per source path.  Used by the
        // rename-candidate filter pipeline (Gate 2: "candidate was not a T₀
        // fact") to reject surviving siblings from consideration as rename
        // targets.  Keyed the same way as `facts_by_path` so the lookup at
        // classify time is O(1).
        let t0_names_by_path = build_t0_names_by_path(&facts);

        let mut rows: Vec<FactAtCommit> = Vec::new();
        for commit in &commits {
            // ── Step 1: read post-commit blobs for all fact-source paths ──────
            // Collect into a HashMap so we can pass them as the blob cache to
            // CommitSymbolIndex::build without double-reading.
            let mut cached_blobs: HashMap<PathBuf, Option<Vec<u8>>> = HashMap::new();
            let mut post_asts: HashMap<PathBuf, Option<RustAst>> = HashMap::new();
            for path in facts_by_path.keys() {
                let blob = pilot.read_blob_at(&commit.sha, path)?;
                let ast = blob.as_ref().and_then(|bytes| RustAst::parse(bytes).ok());
                cached_blobs.insert(path.clone(), blob);
                post_asts.insert(path.clone(), ast);
            }

            // ── Step 2: build commit-local symbol index (when enabled) ────────
            let commit_index: Option<CommitSymbolIndex> = if cfg.skip_symbol_resolution {
                None
            } else {
                let commit_rs_paths = rust_paths_at(&pilot, &commit.sha)?;
                Some(CommitSymbolIndex::build(
                    &pilot,
                    &commit.sha,
                    &commit_rs_paths,
                    &cached_blobs,
                )?)
            };

            // ── Step 3: classify each fact ────────────────────────────────────
            for (path, facts_at_path) in &facts_by_path {
                let post_bytes = cached_blobs.get(path).and_then(|b| b.as_deref());

                // SPEC §5 invariant: an unchanged source file cannot
                // contain a stale fact. When the blob at `commit_sha`
                // is byte-identical to its T₀ counterpart, classify
                // every fact at this path as `Valid` and skip per-fact
                // matching entirely. The bypass lives at the call site
                // (not inside `classify_against_commit`) so it is
                // computed once per `(path, commit)` and visibly
                // sidesteps `matching_post_fact`, `symbol_exists_in_tree`,
                // rename detection, and whitespace/comment diffing.
                // Rationale: benchmarks/provbench/spotcheck/2026-05-12-post-pass3-findings.md.
                let file_byte_identical = match (post_bytes, t0_blobs.get(path)) {
                    (Some(p), Some(t)) => p == t.as_slice(),
                    _ => false,
                };
                if file_byte_identical {
                    for observed in facts_at_path {
                        rows.push(FactAtCommit {
                            fact_id: fact_id(&observed.fact),
                            commit_sha: commit.sha.clone(),
                            label: Label::Valid,
                        });
                    }
                    continue;
                }

                let post_ast = post_asts.get(path).and_then(|a| a.as_ref());
                let t0_names: &HashSet<String> =
                    t0_names_by_path.get(path).unwrap_or(&EMPTY_STRING_SET);
                for observed in facts_at_path {
                    let label = classify_against_commit(
                        &observed.fact,
                        path,
                        post_bytes,
                        post_ast,
                        &observed.t0_span_bytes,
                        observed.test_assertion_ordinal,
                        cfg,
                        commit_index.as_ref(),
                        t0_names,
                        &commit.sha,
                    )?;
                    rows.push(FactAtCommit {
                        fact_id: fact_id(&observed.fact),
                        commit_sha: commit.sha.clone(),
                        label,
                    });
                }
            }
        }
        Ok(rows)
    }
}

/// Shared empty set for paths that have no T₀ facts (avoids allocation).
static EMPTY_STRING_SET: std::sync::LazyLock<HashSet<String>> =
    std::sync::LazyLock::new(HashSet::new);

fn push_observed_facts(
    facts: &mut Vec<ObservedFact>,
    facts_so_far: &mut Vec<Fact>,
    blob: &[u8],
    extracted: impl IntoIterator<Item = Fact>,
) {
    for fact in extracted {
        facts.push(ObservedFact {
            t0_span_bytes: observed_span_bytes(blob, &fact),
            fact: fact.clone(),
            test_assertion_ordinal: None,
        });
        facts_so_far.push(fact);
    }
}

/// Append `Fact::TestAssertion` items to `facts`, computing each fact's
/// zero-based ordinal among same-`(source_path, test_fn)` siblings in
/// the order they arrive from `test_assertion::extract`.
///
/// The ordinal is a structural disambiguator carried only on the
/// private [`ObservedFact`]; it does NOT appear in the serialized
/// [`Fact`] or `fact_id` and therefore does not affect the corpus
/// schema. The counter map is keyed by `(path, test_fn)` so a future
/// refactor that batches facts from multiple source paths into a
/// single call cannot silently alias ordinals across files — the
/// contract is enforced by the key, not by the caller's invocation
/// pattern.
fn push_test_assertion_facts(
    facts: &mut Vec<ObservedFact>,
    facts_so_far: &mut Vec<Fact>,
    blob: &[u8],
    path: &Path,
    extracted: impl IntoIterator<Item = Fact>,
) {
    let mut counters: HashMap<(PathBuf, String), usize> = HashMap::new();
    for fact in extracted {
        let ordinal = match &fact {
            Fact::TestAssertion { test_fn, .. } => {
                let counter = counters
                    .entry((path.to_path_buf(), test_fn.clone()))
                    .or_insert(0);
                let n = *counter;
                *counter += 1;
                Some(n)
            }
            _ => None,
        };
        facts.push(ObservedFact {
            t0_span_bytes: observed_span_bytes(blob, &fact),
            fact: fact.clone(),
            test_assertion_ordinal: ordinal,
        });
        facts_so_far.push(fact);
    }
}

fn group_facts_by_source_path(facts: &[ObservedFact]) -> BTreeMap<PathBuf, Vec<&ObservedFact>> {
    let mut grouped: BTreeMap<PathBuf, Vec<&ObservedFact>> = BTreeMap::new();
    for observed in facts {
        grouped
            .entry(source_path_for(&observed.fact).to_path_buf())
            .or_default()
            .push(observed);
    }
    grouped
}

/// Build a map from source path → set of T₀ qualified names at that path.
///
/// Used by the rename-candidate Gate 2 ("candidate was not a T₀ fact"):
/// before accepting a post-commit symbol as a rename target we verify it
/// did NOT exist at T₀ as an independent fact in the same file.
fn build_t0_names_by_path(facts: &[ObservedFact]) -> BTreeMap<PathBuf, HashSet<String>> {
    let mut by_path: BTreeMap<PathBuf, HashSet<String>> = BTreeMap::new();
    for observed in facts {
        let key = qualified_key_for(&observed.fact).to_string();
        by_path
            .entry(source_path_for(&observed.fact).to_path_buf())
            .or_default()
            .insert(key);
    }
    by_path
}

/// Return the qualified key that uniquely identifies a fact for T₀-presence
/// checks.  Mirrors the per-kind primary key used by `matching_post_fact`:
/// - `FunctionSignature` / `PublicSymbol` / `DocClaim` → `qualified_name`
/// - `Field` → `qualified_path`
/// - `TestAssertion` → `test_fn`
fn qualified_key_for(fact: &Fact) -> &str {
    match fact {
        Fact::FunctionSignature { qualified_name, .. } => qualified_name,
        Fact::Field { qualified_path, .. } => qualified_path,
        Fact::PublicSymbol { qualified_name, .. } => qualified_name,
        Fact::DocClaim { qualified_name, .. } => qualified_name,
        Fact::TestAssertion { test_fn, .. } => test_fn,
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

struct AdHocSpec {
    path: PathBuf,
    t0_sha: String,
}

impl PilotRepoSpec for AdHocSpec {
    fn local_clone_path(&self) -> &Path {
        &self.path
    }
    fn t0_sha(&self) -> &str {
        &self.t0_sha
    }
}

/// Return all `.rs` file paths present in the git tree at `sha`.
fn rust_paths_at(pilot: &Pilot, sha: &str) -> Result<Vec<PathBuf>> {
    Ok(tree_paths_at(pilot, sha)?
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect())
}

fn markdown_paths_at(pilot: &Pilot, sha: &str) -> Result<Vec<PathBuf>> {
    Ok(tree_paths_at(pilot, sha)?
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect())
}

fn tree_paths_at(pilot: &Pilot, sha: &str) -> Result<Vec<PathBuf>> {
    validate_sha_hex(sha)?;
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(pilot.repo_path())
        .args(["ls-tree", "-r", "--name-only", sha])
        .output()
        .with_context(|| format!("git ls-tree {sha}"))?;
    if !out.status.success() {
        anyhow::bail!(
            "git ls-tree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(PathBuf::from)
        .collect())
}

fn is_replay_doc_path(path: &Path, rust_dirs: &std::collections::BTreeSet<PathBuf>) -> bool {
    path.components().count() == 1
        || path
            .parent()
            .map(|parent| rust_dirs.contains(parent))
            .unwrap_or(false)
}

/// Reject anything that isn't exactly 40 lowercase hex characters.
///
/// Used as a defence-in-depth check before passing a SHA into
/// `git ls-tree` so a malformed value cannot reach a subprocess argv.
pub fn validate_sha_hex(sha: &str) -> Result<()> {
    if sha.len() != 40
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        anyhow::bail!("git commit sha must be exactly 40 lowercase hex characters: {sha}");
    }
    Ok(())
}

/// Stable, unique ID for a fact — used as the primary key in output rows.
///
/// Source paths are normalized via [`normalize_path_for_fact_id`] (a pure
/// string transform) so that no absolute filesystem path can ever leak
/// into a `fact_id`, regardless of the user's `pwd` or where the repo
/// lives on disk.
fn fact_id(fact: &Fact) -> String {
    match fact {
        Fact::FunctionSignature {
            qualified_name,
            source_path,
            span,
            ..
        } => {
            format!(
                "FunctionSignature::{qualified_name}::{}::{}",
                normalize_path_for_fact_id(source_path),
                span.line_start
            )
        }
        Fact::Field {
            qualified_path,
            source_path,
            span,
            ..
        } => {
            format!(
                "Field::{qualified_path}::{}::{}",
                normalize_path_for_fact_id(source_path),
                span.line_start
            )
        }
        Fact::PublicSymbol {
            qualified_name,
            source_path,
            span,
            ..
        } => {
            format!(
                "PublicSymbol::{qualified_name}::{}::{}",
                normalize_path_for_fact_id(source_path),
                span.line_start
            )
        }
        Fact::DocClaim {
            qualified_name,
            doc_path,
            mention_span,
            ..
        } => {
            format!(
                "DocClaim::{qualified_name}::{}::{}",
                normalize_path_for_fact_id(doc_path),
                mention_span.line_start
            )
        }
        Fact::TestAssertion {
            test_fn,
            source_path,
            span,
            ..
        } => {
            format!(
                "TestAssertion::{test_fn}::{}::{}",
                normalize_path_for_fact_id(source_path),
                span.line_start
            )
        }
    }
}

// ── Per-commit state ──────────────────────────────────────────────────────────

struct CommitState {
    file_exists: bool,
    post_span_hash: Option<String>,
    structurally_classifiable: bool,
    whitespace_or_comment_only: bool,
    symbol_resolves: bool,
    rename: Option<String>,
}

impl PostCommitState for CommitState {
    fn file_exists(&self) -> bool {
        self.file_exists
    }
    fn symbol_resolves(&self) -> bool {
        self.symbol_resolves
    }
    fn rename_candidate(&self) -> Option<&str> {
        self.rename.as_deref()
    }
    fn post_span_hash(&self) -> Option<&str> {
        self.post_span_hash.as_deref()
    }
    fn whitespace_or_comment_only(&self) -> bool {
        self.whitespace_or_comment_only
    }
    fn structurally_classifiable(&self) -> bool {
        self.structurally_classifiable
    }
}

impl CommitState {
    fn deleted() -> Self {
        Self {
            file_exists: false,
            post_span_hash: None,
            structurally_classifiable: false,
            whitespace_or_comment_only: false,
            symbol_resolves: false,
            rename: None,
        }
    }
}

/// Classify `fact` against the post-commit blob.
///
/// Symbol presence is determined commit-tree-locally:
/// - If `matching_post_fact` finds the symbol at its original path → the
///   symbol is present; compute hash/whitespace deltas and classify.
/// - If not found at the original path AND `commit_index` is `Some`:
///   - If the index says the symbol exists elsewhere in the tree
///     (moved to a different file) → `symbol_resolves = true, no hash`
///     → `classify` returns `NeedsRevalidation` (gray area for LLM).
///   - Otherwise → `symbol_resolves = false` → attempt rename detection
///     → `StaleSourceDeleted` or `StaleSymbolRenamed`.
/// - When `commit_index` is `None` (`skip_symbol_resolution = true`) →
///   bypass cross-file lookup (unit-test mode).
#[allow(clippy::too_many_arguments)]
fn classify_against_commit(
    fact: &Fact,
    path: &Path,
    post_blob: Option<&[u8]>,
    post_ast: Option<&RustAst>,
    t0_span_bytes: &[u8],
    test_assertion_ordinal: Option<usize>,
    cfg: &ReplayConfig,
    commit_index: Option<&CommitSymbolIndex>,
    t0_qualified_names: &HashSet<String>,
    commit_sha: &str,
) -> Result<Label> {
    let state = match post_blob {
        // File was deleted at this commit.
        None => CommitState::deleted(),

        Some(post_bytes) => {
            let observed_hash = observed_hash_for(fact);

            // Search the post-commit file for the same fact kind/key.
            let post_fact = match_post::matching_post_fact(
                fact,
                path,
                post_bytes,
                post_ast,
                test_assertion_ordinal,
                commit_sha,
            )?;

            match post_fact {
                Some((post_span, post_hash)) => {
                    // Symbol found at its original path — compute deltas.
                    let after_bytes = &post_bytes[post_span.byte_range.clone()];
                    let ws_only = is_whitespace_or_comment_only(t0_span_bytes, after_bytes);
                    // Any signature-level hash difference is structurally classifiable.
                    let structural = post_hash != observed_hash;
                    CommitState {
                        file_exists: true,
                        post_span_hash: Some(post_hash),
                        structurally_classifiable: structural,
                        whitespace_or_comment_only: ws_only,
                        symbol_resolves: true,
                        rename: None,
                    }
                }
                None => {
                    // Symbol not found at its original path in the post-commit AST.
                    if cfg.skip_symbol_resolution {
                        // Unit-test mode: no cross-file or rename detection.
                        CommitState {
                            file_exists: true,
                            post_span_hash: None,
                            structurally_classifiable: false,
                            whitespace_or_comment_only: false,
                            symbol_resolves: false,
                            rename: None,
                        }
                    } else if let Some(index) = commit_index {
                        // Commit-tree-local resolution: check whether the symbol
                        // exists anywhere in this commit's tree.
                        //
                        // Case 1 — symbol exists at a different path: the fact's
                        // source was moved to another file.  This is a gray area
                        // for the LLM (moved vs. renamed vs. coincidental name
                        // match), so we route to NeedsRevalidation without
                        // triggering StaleSymbolRenamed.
                        //
                        // Case 2 — symbol absent from the commit tree entirely:
                        // the qualified name is gone from every file.  Run the
                        // typed rename pipeline against the same-file post-commit
                        // AST to detect within-file renames (e.g. `old_name` →
                        // `new_name` in the same module/impl block).  If a
                        // candidate passes all four gates → StaleSymbolRenamed;
                        // otherwise → StaleSourceDeleted.
                        if index.symbol_exists_in_tree(fact) {
                            CommitState {
                                file_exists: true,
                                post_span_hash: None,
                                structurally_classifiable: false,
                                whitespace_or_comment_only: false,
                                symbol_resolves: true,
                                rename: None,
                            }
                        } else {
                            let candidates = match_post::rename_candidates_for_typed(
                                fact, path, post_bytes, post_ast,
                            );
                            let origin = RenameOrigin::new(qualified_key_for(fact), t0_span_bytes);
                            let rename = rename_candidate_typed(
                                &origin,
                                &candidates,
                                t0_qualified_names,
                                0.6,
                            );
                            CommitState {
                                file_exists: true,
                                post_span_hash: None,
                                structurally_classifiable: false,
                                whitespace_or_comment_only: false,
                                symbol_resolves: false,
                                rename,
                            }
                        }
                    } else {
                        // `skip_symbol_resolution = false` but `commit_index` is
                        // `None`.  This cannot happen in practice: `run_inner`
                        // always builds the index when `skip_symbol_resolution`
                        // is `false`.  Treat as deleted (no rename detection)
                        // rather than panicking so behaviour is defined.
                        CommitState {
                            file_exists: true,
                            post_span_hash: None,
                            structurally_classifiable: false,
                            whitespace_or_comment_only: false,
                            symbol_resolves: false,
                            rename: None,
                        }
                    }
                }
            }
        }
    };

    Ok(classify(fact, &state))
}

fn observed_span_bytes(blob: &[u8], fact: &Fact) -> Vec<u8> {
    blob[span_for(fact).byte_range.clone()].to_vec()
}

fn span_for(fact: &Fact) -> &Span {
    match fact {
        Fact::FunctionSignature { span, .. } => span,
        Fact::Field { span, .. } => span,
        Fact::PublicSymbol { span, .. } => span,
        Fact::DocClaim { mention_span, .. } => mention_span,
        Fact::TestAssertion { span, .. } => span,
    }
}

fn source_path_for(fact: &Fact) -> &Path {
    match fact {
        Fact::FunctionSignature { source_path, .. } => source_path,
        Fact::Field { source_path, .. } => source_path,
        Fact::PublicSymbol { source_path, .. } => source_path,
        Fact::DocClaim { doc_path, .. } => doc_path,
        Fact::TestAssertion { source_path, .. } => source_path,
    }
}

fn observed_hash_for(fact: &Fact) -> &str {
    match fact {
        Fact::FunctionSignature { content_hash, .. } => content_hash,
        Fact::Field { content_hash, .. } => content_hash,
        Fact::PublicSymbol { content_hash, .. } => content_hash,
        Fact::DocClaim { mention_hash, .. } => mention_hash,
        Fact::TestAssertion { content_hash, .. } => content_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn commit_all(repo: &Path, message: &str) {
        git(repo, &["add", "."]);
        git(
            repo,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-m",
                message,
            ],
        );
    }

    #[test]
    fn replay_reads_each_source_blob_once_per_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        git(repo, &["init", "--initial-branch=main"]);
        std::fs::create_dir(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("Cargo.toml"),
            b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join("src/lib.rs"),
            b"pub fn a() -> i32 { 1 }\npub fn b() -> i32 { 2 }\npub fn c() -> i32 { 3 }\npub fn d() -> i32 { 4 }\npub fn e() -> i32 { 5 }\n",
        )
        .unwrap();
        commit_all(repo, "init");
        let t0 = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();

        std::fs::write(
            repo.join("src/lib.rs"),
            b"pub fn a() -> i32 { 10 }\npub fn b() -> i32 { 2 }\npub fn c() -> i32 { 3 }\npub fn d() -> i32 { 4 }\npub fn e() -> i32 { 5 }\n",
        )
        .unwrap();
        commit_all(repo, "body tweak one");
        std::fs::write(
            repo.join("src/lib.rs"),
            b"pub fn a() -> i32 { 10 }\npub fn b() -> i32 { 20 }\npub fn c() -> i32 { 3 }\npub fn d() -> i32 { 4 }\npub fn e() -> i32 { 5 }\n",
        )
        .unwrap();
        commit_all(repo, "body tweak two");

        let cfg = ReplayConfig {
            repo_path: repo.to_path_buf(),
            t0_sha: t0,
            skip_symbol_resolution: true,
        };
        let rows = Replay::run(&cfg).unwrap();
        assert_eq!(rows.len(), 30);
        let reads = crate::repo::read_blob_at_call_count();
        assert!(
            reads <= 3,
            "expected at most one blob read per commit for one source file, got {reads}"
        );
    }
}
