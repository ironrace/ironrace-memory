//! ProvBench Phase 0b mechanical labeler.
pub mod tooling;

pub fn labeler_stamp() -> String {
    option_env!("PROVBENCH_LABELER_GIT_SHA")
        .unwrap_or("unstamped")
        .to_string()
}
