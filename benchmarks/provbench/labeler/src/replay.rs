//! Per-commit replay: read blobs at each commit, compute post-commit state
//! per fact, classify, emit `FactAtCommit` rows.
//!
//! v1 limitation: only `Fact::FunctionSignature` facts are extracted at T₀.
//! Other extractors (Field, PublicSymbol, DocClaim, TestAssertion) will be
//! wired in a follow-up task once the integration surface is stable.

use crate::ast::RustAst;
use crate::diff::{is_whitespace_or_comment_only, rename_candidate};
use crate::facts::{function_signature, Fact};
use crate::label::{classify, Label, PostCommitState};
use crate::repo::{CommitRef, Pilot, PilotRepoSpec};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Public surface ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactAtCommit {
    /// `"{kind}::{qualified}::{source}::{line_start}"`
    pub fact_id: String,
    pub commit_sha: String,
    pub label: Label,
}

pub struct ReplayConfig {
    pub repo_path: PathBuf,
    pub t0_sha: String,
    /// When `true`, the driver does not consult rust-analyzer; symbol
    /// resolution is approximated as "the bound source path still exists AND
    /// the function signature still appears at any location in the post-commit
    /// AST".  Used by unit tests; production runs must set this `false`.
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

        // Extract the fact set at T₀ across every .rs file present at T₀.
        // v1: FunctionSignature facts only.
        let mut facts: Vec<Fact> = Vec::new();
        for path in rust_paths_at(&pilot, &cfg.t0_sha)? {
            if let Some(blob) = pilot.read_blob_at(&cfg.t0_sha, &path)? {
                let ast = RustAst::parse(&blob)
                    .with_context(|| format!("parse {} @ T0", path.display()))?;
                facts.extend(function_signature::extract(&ast, &path));
            }
        }

        let mut rows: Vec<FactAtCommit> = Vec::new();
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
        .filter(|l| l.ends_with(".rs"))
        .map(PathBuf::from)
        .collect())
}

/// Stable, unique ID for a fact — used as the primary key in output rows.
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
                source_path.display(),
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
                source_path.display(),
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
                source_path.display(),
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
                doc_path.display(),
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
                source_path.display(),
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

/// Classify `fact` against the post-commit blob at `commit_sha`.
fn classify_against_commit(
    pilot: &Pilot,
    fact: &Fact,
    commit_sha: &str,
    cfg: &ReplayConfig,
) -> Result<Label> {
    let path = source_path_for(fact);
    let blob = pilot.read_blob_at(commit_sha, path)?;

    let state = match blob {
        // File was deleted at this commit.
        None => CommitState::deleted(),

        Some(post_bytes) => {
            let observed_hash = observed_hash_for(fact);
            let qualified_name = qualified_name_for(fact);

            // Parse the post-commit blob and search for the same symbol.
            let post_ast = RustAst::parse(&post_bytes).ok();
            let post_sig = post_ast.as_ref().and_then(|ast| {
                function_signature::extract(ast, path).find_map(|f| match f {
                    Fact::FunctionSignature {
                        qualified_name: q,
                        span,
                        content_hash,
                        ..
                    } if q == qualified_name => Some((span, content_hash)),
                    _ => None,
                })
            });

            match post_sig {
                Some((post_span, post_hash)) => {
                    // Symbol found — compute whitespace/structural deltas.
                    let before_bytes = t0_span_bytes(pilot, &cfg.t0_sha, path, fact)?;
                    let after_bytes = post_bytes[post_span.byte_range.clone()].to_vec();
                    let ws_only = is_whitespace_or_comment_only(&before_bytes, &after_bytes);
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
                    // Symbol not found in post-commit AST.
                    if cfg.skip_symbol_resolution {
                        // Unit-test mode: no rename detection.
                        CommitState {
                            file_exists: true,
                            post_span_hash: None,
                            structurally_classifiable: false,
                            whitespace_or_comment_only: false,
                            symbol_resolves: false,
                            rename: None,
                        }
                    } else {
                        // Production mode: attempt rename detection.
                        let candidates: Vec<(String, Vec<u8>)> = post_ast
                            .iter()
                            .flat_map(|ast| {
                                function_signature::extract(ast, path).filter_map(|f| match f {
                                    Fact::FunctionSignature {
                                        qualified_name: q,
                                        span,
                                        ..
                                    } => {
                                        let bytes = post_bytes[span.byte_range].to_vec();
                                        Some((q, bytes))
                                    }
                                    _ => None,
                                })
                            })
                            .collect();
                        let before_bytes = t0_span_bytes(pilot, &cfg.t0_sha, path, fact)?;
                        let rename = rename_candidate(&before_bytes, &candidates, 0.6);
                        CommitState {
                            file_exists: true,
                            post_span_hash: None,
                            structurally_classifiable: false,
                            whitespace_or_comment_only: false,
                            symbol_resolves: false,
                            rename,
                        }
                    }
                }
            }
        }
    };

    Ok(classify(fact, &state))
}

/// Extract the span bytes from the T₀ blob for `fact`.
fn t0_span_bytes(pilot: &Pilot, t0: &str, path: &Path, fact: &Fact) -> Result<Vec<u8>> {
    let blob = pilot
        .read_blob_at(t0, path)?
        .with_context(|| format!("T0 blob missing: {}", path.display()))?;
    let span = match fact {
        Fact::FunctionSignature { span, .. }
        | Fact::Field { span, .. }
        | Fact::PublicSymbol { span, .. }
        | Fact::TestAssertion { span, .. } => span,
        Fact::DocClaim { mention_span, .. } => mention_span,
    };
    Ok(blob[span.byte_range.clone()].to_vec())
}

fn source_path_for(fact: &Fact) -> &Path {
    match fact {
        Fact::FunctionSignature { source_path, .. }
        | Fact::Field { source_path, .. }
        | Fact::PublicSymbol { source_path, .. }
        | Fact::TestAssertion { source_path, .. } => source_path,
        Fact::DocClaim { doc_path, .. } => doc_path,
    }
}

fn observed_hash_for(fact: &Fact) -> &str {
    match fact {
        Fact::FunctionSignature { content_hash, .. }
        | Fact::Field { content_hash, .. }
        | Fact::PublicSymbol { content_hash, .. }
        | Fact::TestAssertion { content_hash, .. } => content_hash,
        Fact::DocClaim { mention_hash, .. } => mention_hash,
    }
}

fn qualified_name_for(fact: &Fact) -> &str {
    match fact {
        Fact::FunctionSignature { qualified_name, .. }
        | Fact::PublicSymbol { qualified_name, .. }
        | Fact::DocClaim { qualified_name, .. } => qualified_name,
        Fact::Field { qualified_path, .. } => qualified_path,
        Fact::TestAssertion { test_fn, .. } => test_fn,
    }
}
