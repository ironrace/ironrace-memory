//! ProvBench Phase 0b mechanical labeler.
//!
//! Frozen contract: `benchmarks/provbench/SPEC.md`. This crate does not
//! depend on `ironmem` or any workspace crate — Phase 0 lives outside the
//! system code so the corpus and labeler can be released as a standalone
//! reproducible artifact.

pub mod ast;
pub mod diff;
pub mod facts;
pub mod label;
pub mod output;
pub mod replay;
pub mod repo;
pub mod resolve;
pub mod spotcheck;
pub mod tooling;

pub fn labeler_stamp() -> String {
    let sha = option_env!("PROVBENCH_LABELER_GIT_SHA").unwrap_or("unstamped");
    match option_env!("PROVBENCH_LABELER_DIRTY") {
        Some("true") if sha != "unstamped" => format!("{sha}-dirty"),
        _ => sha.to_string(),
    }
}
