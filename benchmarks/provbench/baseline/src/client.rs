//! Anthropic Messages API client. Task 6 carved out only `Usage` so the
//! budget meter can take a typed parameter; the full HTTP client (retries,
//! cache_control, parse-error addendum) lands in Task 7.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
    #[serde(default)]
    pub cache_read_input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
}
