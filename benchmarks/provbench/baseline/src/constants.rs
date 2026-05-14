//! Shared constants for the ProvBench Phase 0c LLM-as-invalidator baseline.
//!
//! Anything pinned by SPEC §6.2 / §6.3 lives here so call-sites reference a
//! single source of truth. Edits must be matched by a SPEC §11 entry.

/// Anthropic model identifier pinned in SPEC §6.2.
pub const MODEL_ID: &str = "claude-sonnet-4-6";

/// API snapshot date pinned in SPEC §6.2.
pub const MODEL_SNAPSHOT_DATE: &str = "2026-05-09";

/// Sampling temperature pinned in SPEC §6.2.
pub const TEMPERATURE: f32 = 0.0;

/// Max output tokens per call (SPEC §6.2).
pub const MAX_TOKENS: u32 = 4096;

/// Maximum facts per prompt (SPEC §6.2 batching limit).
pub const MAX_FACTS_PER_PROMPT: usize = 32;

/// Maximum retries on transient errors (SPEC §6.2).
pub const MAX_TRANSIENT_RETRIES: u32 = 2;

/// Phase 0c pre-registered budget cap in USD (SPEC §6.2).
pub const PHASE_0C_BUDGET_USD: f64 = 250.0;

/// 2026-05-09 token-price snapshot: USD per 1M input tokens (SPEC §6.2).
pub const PRICE_INPUT_PER_M: f64 = 3.00;

/// 2026-05-09 token-price snapshot: USD per 1M output tokens (SPEC §6.2).
pub const PRICE_OUTPUT_PER_M: f64 = 15.00;
