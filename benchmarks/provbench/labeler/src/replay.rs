//! Per-commit replay: read blobs at each commit, compute post-commit state
//! per fact, classify, emit `FactAtCommit` rows.

use crate::ast::{
    spans::{content_hash, Span},
    RustAst,
};
use crate::diff::{is_whitespace_or_comment_only, rename_candidate};
use crate::facts::{doc_claim, field, function_signature, symbol_existence, test_assertion, Fact};
use crate::label::{classify, Label, PostCommitState};
use crate::repo::{CommitRef, Pilot, PilotRepoSpec};
use crate::resolve::{rust_analyzer::RustAnalyzer, SymbolResolver};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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

struct ObservedFact {
    fact: Fact,
    t0_span_bytes: Vec<u8>,
}

impl Replay {
    pub fn run(cfg: &ReplayConfig) -> Result<Vec<FactAtCommit>> {
        let mut resolver: Option<RustAnalyzer> = if cfg.skip_symbol_resolution {
            None
        } else {
            crate::tooling::resolve_from_env()?;
            // TODO(phase-0b-pilot): replay production labeling against a
            // per-commit worktree before each rust-analyzer query so the LSP
            // view matches the commit being classified.
            Some(RustAnalyzer::spawn(&cfg.repo_path)?)
        };

        let pilot = Pilot::open(&AdHocSpec {
            path: cfg.repo_path.clone(),
            t0_sha: cfg.t0_sha.clone(),
        })?;
        let commits: Vec<CommitRef> = pilot.walk_first_parent()?.collect();

        // Extract the fact set at T₀ across every .rs file present at T₀.
        let mut facts: Vec<ObservedFact> = Vec::new();
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
                push_observed_facts(&mut facts, &mut facts_so_far, &blob, test_facts);
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
                let doc_facts: Vec<Fact> =
                    doc_claim::extract(&blob, &path, &facts_so_far).collect();
                push_observed_facts(&mut facts, &mut facts_so_far, &blob, doc_facts);
            }
        }

        #[cfg(test)]
        crate::repo::reset_read_blob_at_call_count();

        let facts_by_path = group_facts_by_source_path(&facts);
        let mut rows: Vec<FactAtCommit> = Vec::new();
        for commit in &commits {
            for (path, facts_at_path) in &facts_by_path {
                let post_bytes = pilot.read_blob_at(&commit.sha, path)?;
                let post_ast = post_bytes
                    .as_ref()
                    .and_then(|bytes| RustAst::parse(bytes).ok());
                for observed in facts_at_path {
                    let resolver_ref = resolver
                        .as_mut()
                        .map(|resolver| resolver as &mut dyn SymbolResolver);
                    let label = classify_against_commit(
                        &observed.fact,
                        path,
                        post_bytes.as_deref(),
                        post_ast.as_ref(),
                        &observed.t0_span_bytes,
                        cfg,
                        resolver_ref,
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
    fact: &Fact,
    path: &Path,
    post_blob: Option<&[u8]>,
    post_ast: Option<&RustAst>,
    t0_span_bytes: &[u8],
    cfg: &ReplayConfig,
    resolver: Option<&mut dyn SymbolResolver>,
) -> Result<Label> {
    let state = match post_blob {
        // File was deleted at this commit.
        None => CommitState::deleted(),

        Some(post_bytes) => {
            let observed_hash = observed_hash_for(fact);

            // Search the post-commit file for the same fact kind/key.
            let post_fact = matching_post_fact(fact, path, post_bytes, post_ast);

            match post_fact {
                Some((post_span, post_hash)) => {
                    // Symbol found — compute whitespace/structural deltas.
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
                        if let Some(resolver) = resolver {
                            let resolved = resolver.resolve(qualified_name_for(fact))?;
                            if resolved.is_some() {
                                return Ok(classify(
                                    fact,
                                    &CommitState {
                                        file_exists: true,
                                        post_span_hash: None,
                                        structurally_classifiable: false,
                                        whitespace_or_comment_only: false,
                                        symbol_resolves: true,
                                        rename: None,
                                    },
                                ));
                            }
                        }
                        // Production mode: attempt rename detection.
                        let candidates = rename_candidates_for(fact, path, post_bytes, post_ast);
                        let rename = rename_candidate(t0_span_bytes, &candidates, 0.6);
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

fn observed_span_bytes(blob: &[u8], fact: &Fact) -> Vec<u8> {
    blob[span_for(fact).byte_range.clone()].to_vec()
}

fn matching_post_fact(
    fact: &Fact,
    path: &Path,
    post_bytes: &[u8],
    post_ast: Option<&RustAst>,
) -> Option<(Span, String)> {
    match fact {
        Fact::FunctionSignature { qualified_name, .. } => post_ast.and_then(|ast| {
            function_signature::extract(ast, path).find_map(|f| match f {
                Fact::FunctionSignature {
                    qualified_name: q,
                    span,
                    content_hash,
                    ..
                } if q == *qualified_name => Some((span, content_hash)),
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
        }),
        Fact::Field { qualified_path, .. } => post_ast.and_then(|ast| {
            field::extract(ast, path).find_map(|f| match f {
                Fact::Field {
                    qualified_path: q,
                    span,
                    content_hash,
                    ..
                } if q == *qualified_path => Some((span, content_hash)),
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
        }),
        Fact::PublicSymbol { qualified_name, .. } => post_ast.and_then(|ast| {
            symbol_existence::extract(ast, path).find_map(|f| match f {
                Fact::PublicSymbol {
                    qualified_name: q,
                    span,
                    content_hash,
                    ..
                } if q == *qualified_name => Some((span, content_hash)),
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
        }),
        Fact::DocClaim { mention_span, .. } => {
            let range = mention_span.byte_range.clone();
            if range.end > post_bytes.len() {
                return None;
            }
            Some((
                Span {
                    byte_range: range.clone(),
                    line_start: mention_span.line_start,
                    line_end: mention_span.line_end,
                },
                content_hash(&post_bytes[range]),
            ))
        }
        Fact::TestAssertion { test_fn, .. } => post_ast.and_then(|ast| {
            test_assertion::extract(ast, path, &[]).find_map(|f| match f {
                Fact::TestAssertion {
                    test_fn: q,
                    span,
                    content_hash,
                    ..
                } if q == *test_fn => Some((span, content_hash)),
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
        }),
    }
}

fn rename_candidates_for(
    fact: &Fact,
    path: &Path,
    post_bytes: &[u8],
    post_ast: Option<&RustAst>,
) -> Vec<(String, Vec<u8>)> {
    let Some(ast) = post_ast else {
        return Vec::new();
    };
    match fact {
        Fact::FunctionSignature { .. } => function_signature::extract(ast, path)
            .filter_map(|f| match f {
                Fact::FunctionSignature {
                    qualified_name,
                    span,
                    ..
                } => Some((qualified_name, post_bytes[span.byte_range].to_vec())),
                Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
            .collect(),
        Fact::Field { .. } => field::extract(ast, path)
            .filter_map(|f| match f {
                Fact::Field {
                    qualified_path,
                    span,
                    ..
                } => Some((qualified_path, post_bytes[span.byte_range].to_vec())),
                Fact::FunctionSignature { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
            .collect(),
        Fact::PublicSymbol { .. } => symbol_existence::extract(ast, path)
            .filter_map(|f| match f {
                Fact::PublicSymbol {
                    qualified_name,
                    span,
                    ..
                } => Some((qualified_name, post_bytes[span.byte_range].to_vec())),
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::DocClaim { .. }
                | Fact::TestAssertion { .. } => None,
            })
            .collect(),
        Fact::DocClaim { .. } => Vec::new(),
        Fact::TestAssertion { .. } => test_assertion::extract(ast, path, &[])
            .filter_map(|f| match f {
                Fact::TestAssertion { test_fn, span, .. } => {
                    Some((test_fn, post_bytes[span.byte_range].to_vec()))
                }
                Fact::FunctionSignature { .. }
                | Fact::Field { .. }
                | Fact::PublicSymbol { .. }
                | Fact::DocClaim { .. } => None,
            })
            .collect(),
    }
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

fn qualified_name_for(fact: &Fact) -> &str {
    match fact {
        Fact::FunctionSignature { qualified_name, .. } => qualified_name,
        Fact::Field { qualified_path, .. } => qualified_path,
        Fact::PublicSymbol { qualified_name, .. } => qualified_name,
        Fact::DocClaim { qualified_name, .. } => qualified_name,
        Fact::TestAssertion { test_fn, .. } => test_fn,
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
