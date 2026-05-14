//! Shared scoring math for ProvBench (extracted from `provbench-baseline`).
//!
//! Both `provbench-baseline` and `provbench-phase1` depend on this crate.
//! SPEC §7 math (Wilson LB, three-way scoring, Cohen's κ + bootstrap CI,
//! latency, cost) lives here.

pub mod compare;
pub mod constants;
pub mod manifest;
pub mod metrics;
pub mod predictions;
pub mod report;

pub use predictions::PredictionRow;
