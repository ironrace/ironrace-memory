//! Per-commit symbol index built from tree-sitter ASTs.
//!
//! [`CommitSymbolIndex`] answers "does a fact's qualified symbol exist
//! anywhere in this commit's tree?" using only blobs from that commit —
//! never the working tree or HEAD.  It is built once per commit, before
//! the per-fact classification loop, so each blob is **read** at most once.
//!
//! # Blob-read budget
//! `build` accepts a map of already-read blobs (keyed by repo-relative
//! path) so callers can reuse reads that happened earlier in the same
//! commit iteration.  Paths absent from `cached_blobs` are fetched via
//! [`Pilot::read_blob_at`].

use crate::ast::RustAst;
use crate::facts::{field, function_signature, symbol_existence, test_assertion, Fact};
use crate::repo::Pilot;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Per-commit, kind-partitioned set of qualified names present in the tree.
///
/// Only `.rs` blobs are indexed; markdown blobs are not parsed here because
/// `DocClaim` resolution is byte-range–based and does not benefit from a
/// tree-wide symbol index.
pub struct CommitSymbolIndex {
    function_names: HashSet<String>,
    field_names: HashSet<String>,
    symbol_names: HashSet<String>,
    test_names: HashSet<String>,
}

impl CommitSymbolIndex {
    /// Build the index for `commit_sha` over all `.rs` paths in `rs_paths`.
    ///
    /// `cached_blobs` is a map of blobs already read in the same commit
    /// iteration (keyed by repo-relative path).  Paths present in the map
    /// are used directly; absent paths are fetched via `pilot.read_blob_at`.
    /// Deleted paths (`None` in the map, or returning `None` from the pilot)
    /// are skipped.
    pub fn build(
        pilot: &Pilot,
        commit_sha: &str,
        rs_paths: &[PathBuf],
        cached_blobs: &HashMap<PathBuf, Option<Vec<u8>>>,
    ) -> Result<Self> {
        let mut function_names = HashSet::new();
        let mut field_names = HashSet::new();
        let mut symbol_names = HashSet::new();
        let mut test_names = HashSet::new();

        for path in rs_paths {
            // Reuse a cached blob if available; only call read_blob_at when
            // the path was not already fetched for this commit.
            // Borrow cached bytes in-place to avoid cloning when possible.
            let fetched: Option<Vec<u8>>;
            let bytes: &[u8] = match cached_blobs.get(path) {
                Some(Some(cached)) => cached,
                Some(None) => continue, // path was deleted at this commit
                None => {
                    fetched = pilot.read_blob_at(commit_sha, path)?;
                    match &fetched {
                        Some(b) => b,
                        None => continue,
                    }
                }
            };
            let Ok(ast) = RustAst::parse(bytes) else {
                continue;
            };
            for fact in function_signature::extract(&ast, path) {
                if let Fact::FunctionSignature { qualified_name, .. } = fact {
                    function_names.insert(qualified_name);
                }
            }
            for fact in field::extract(&ast, path) {
                if let Fact::Field { qualified_path, .. } = fact {
                    field_names.insert(qualified_path);
                }
            }
            for fact in symbol_existence::extract(&ast, path) {
                if let Fact::PublicSymbol { qualified_name, .. } = fact {
                    symbol_names.insert(qualified_name);
                }
            }
            // test_assertion::extract needs a prior-facts slice; pass empty
            // since we only need the test function names (not cross-refs).
            for fact in test_assertion::extract(&ast, path, &[]) {
                if let Fact::TestAssertion { test_fn, .. } = fact {
                    test_names.insert(test_fn);
                }
            }
        }

        Ok(Self {
            function_names,
            field_names,
            symbol_names,
            test_names,
        })
    }

    /// Returns `true` if a same-kind, same-qualified-name symbol for `fact`
    /// exists anywhere in this commit's tree (including at the fact's
    /// original source path).
    ///
    /// `DocClaim` always returns `false` — doc claims are byte-range–anchored
    /// and are not indexed here.
    pub fn symbol_exists_for_fact(&self, fact: &Fact) -> bool {
        match fact {
            Fact::FunctionSignature { qualified_name, .. } => {
                self.function_names.contains(qualified_name.as_str())
            }
            Fact::Field { qualified_path, .. } => {
                self.field_names.contains(qualified_path.as_str())
            }
            Fact::PublicSymbol { qualified_name, .. } => {
                self.symbol_names.contains(qualified_name.as_str())
            }
            Fact::TestAssertion { test_fn, .. } => self.test_names.contains(test_fn.as_str()),
            Fact::DocClaim { .. } => false,
        }
    }

    /// Returns `true` if a same-kind, same-qualified Rust symbol exists
    /// anywhere in the commit's `.rs` tree.
    ///
    /// The index is path-agnostic (it tracks only qualified names, not which
    /// file each name comes from), so this method simply checks whether the
    /// symbol is present anywhere in the commit tree.  The "elsewhere"
    /// guarantee — that the symbol is absent from its original source path —
    /// comes entirely from the caller's control flow: this method should only
    /// be invoked after `matching_post_fact` has already returned `None`.
    pub fn symbol_exists_in_tree(&self, fact: &Fact) -> bool {
        self.symbol_exists_for_fact(fact)
    }
}
