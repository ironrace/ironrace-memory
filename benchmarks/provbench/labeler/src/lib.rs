//! ProvBench Phase 0b mechanical labeler.
//!
//! Produces the `fact_at_commit` corpus per SPEC §3–§5. Given a pilot repo
//! and a T₀ commit, the labeler extracts the closed enum of [`facts::Fact`]
//! kinds at T₀ and replays each subsequent first-parent commit, classifying
//! every fact under the [`label::Label`] enum via the rule engine in
//! [`label::classify`]. Output is byte-deterministic JSONL stamped with the
//! labeler's own git SHA (see [`labeler_stamp`]).
//!
//! Frozen contract: `benchmarks/provbench/SPEC.md`. This crate does not
//! depend on `ironmem` or any workspace crate — Phase 0 lives outside the
//! system code so the corpus and labeler can be released as a standalone
//! reproducible artifact.
//!
//! Module map:
//! - [`ast`]: tree-sitter Rust parser wrapper used by every extractor.
//! - [`diff`]: structural-equivalence and rename-candidate helpers used by
//!   the rule engine to discriminate trivia from real source changes.
//! - [`facts`]: per-kind extractors that produce the T₀ fact set.
//! - [`label`]: the SPEC §5 first-match-wins rule engine.
//! - [`lang`]: language enum + per-path dispatch for source files.
//! - [`output`]: deterministic JSONL writer with labeler-SHA stamping.
//! - [`replay`]: per-commit replay driver — reads blobs at each commit,
//!   classifies, and emits `FactAtCommit` rows.
//! - [`repo`]: pilot repo handle (gix) and first-parent commit walker.
//! - [`resolve`]: language-agnostic symbol resolver trait + the legacy
//!   rust-analyzer LSP backend retained for ignored RA tooling tests and
//!   future semantic-resolution work.
//! - [`spotcheck`]: stratified deterministic sampler and Wilson lower-bound
//!   reporter for SPEC §9.1 acceptance.
//! - [`tooling`]: pinned-binary verification per SPEC §13.1.

pub mod ast;
pub mod diff;
pub mod facts;
pub mod label;
pub mod lang;
pub mod output;
pub mod replay;
pub mod repo;
pub mod resolve;
pub mod spotcheck;
pub mod tooling;

/// Compile-time stamp of the labeler git SHA, with `-dirty` suffix when the
/// build tree had uncommitted changes.
///
/// Set by `build.rs` via the `PROVBENCH_LABELER_GIT_SHA` /
/// `PROVBENCH_LABELER_DIRTY` env vars. Returns `"unstamped"` for
/// non-build-script builds (e.g. `cargo doc`). Embedded in every output row
/// so a corpus file can be tied back to the exact labeler revision that
/// produced it.
pub fn labeler_stamp() -> String {
    let sha = option_env!("PROVBENCH_LABELER_GIT_SHA").unwrap_or("unstamped");
    match option_env!("PROVBENCH_LABELER_DIRTY") {
        Some("true") if sha != "unstamped" => format!("{sha}-dirty"),
        _ => sha.to_string(),
    }
}
