//! SPEC immutables consumed by the shared scorer.
//!
//! Mirrored from `provbench-baseline::constants` — only the values the
//! scorer needs (model identity for the `metrics.json` header + the
//! deterministic bootstrap seed). The baseline crate keeps the
//! authoritative copy; both must stay in lock-step with the SPEC
//! §6.2 / §15 snapshot.

/// Anthropic model identity (SPEC §6.2 pin).
pub const MODEL_ID: &str = "claude-sonnet-4-6";
pub const MODEL_SNAPSHOT_DATE: &str = "2026-05-09";

/// Default sample seed (same constant the labeler uses for spotcheck
/// draws). Used as the bootstrap-CI seed for Cohen's κ so the reported
/// interval is reproducible.
pub const DEFAULT_SEED: u64 = 0xC0DE_BABE_DEAD_BEEF;
