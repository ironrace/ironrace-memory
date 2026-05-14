//! Anthropic Messages API client. Retry + cache_control + parse-error addendum.

use crate::constants::*;
use crate::prompt::{ContentBlock, PARSE_RETRY_ADDENDUM};
use anyhow::{Context, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::time::Duration;

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

impl Usage {
    pub fn add_assign(&mut self, other: &Usage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.cache_creation_input_tokens = self
            .cache_creation_input_tokens
            .saturating_add(other.cache_creation_input_tokens);
        self.cache_read_input_tokens = self
            .cache_read_input_tokens
            .saturating_add(other.cache_read_input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Decision {
    pub id: String,
    pub decision: String, // "valid" | "stale" | "needs_revalidation"
}

#[derive(Debug, Clone)]
pub struct BatchResponse {
    pub decisions: Vec<Decision>,
    pub usage: Usage,
    pub request_id: String,
    pub wall_ms: u64,
}

/// Returned by [`AnthropicClient::score_batch`] when the model's response
/// text fails to parse as `Vec<Decision>` on both the original attempt
/// and the addendum-retry attempt. Carries the raw second-attempt text
/// and request id so the runner can persist a diagnostic sidecar entry
/// and skip the batch instead of aborting the whole run.
#[derive(Debug, thiserror::Error)]
#[error("response parse failed after addendum retry: {err_msg}")]
pub struct ParseFailureError {
    pub raw_text: String,
    pub request_id: String,
    pub err_msg: String,
}

pub struct AnthropicClient {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl AnthropicClient {
    pub fn from_env() -> Result<Self> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("IRONMEM_ANTHROPIC_API_KEY"))
            .context("ANTHROPIC_API_KEY (or IRONMEM_ANTHROPIC_API_KEY) must be set")?;
        Ok(Self::with_base_url("https://api.anthropic.com".into(), key))
    }

    pub fn with_base_url(base_url: String, api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            api_key,
        }
    }

    /// Dispatch one batch of prompt blocks.
    ///
    /// Two independent retry axes:
    ///   - `transient_attempt` — up to two retries for 5xx/429/network
    ///     errors. Bounded so a flapping upstream cannot exhaust the
    ///     runtime budget.
    ///   - `parse_retried` — at most ONE retry that appends
    ///     [`PARSE_RETRY_ADDENDUM`] after a malformed (non-JSON / wrong-
    ///     shape) model response. This axis is independent of the
    ///     transient axis: a parse retry does not consume a transient
    ///     slot, and a transient retry does not consume the parse-retry
    ///     slot.
    pub async fn score_batch(&self, blocks: Vec<ContentBlock>) -> Result<BatchResponse> {
        let started = std::time::Instant::now();
        let mut attempt_blocks = blocks;
        let mut transient_attempt: usize = 0;
        let mut parse_retried = false;
        let mut cumulative_usage = Usage::default();
        const MAX_TRANSIENT_RETRIES: usize = 2;

        loop {
            let body = build_request_body(&attempt_blocks);
            let resp = self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    if transient_attempt < MAX_TRANSIENT_RETRIES {
                        let backoff = backoff_for(transient_attempt);
                        transient_attempt += 1;
                        tracing::warn!(
                            "transient network error: {e}; retrying after {:?}",
                            backoff
                        );
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    return Err(e.into());
                }
            };

            let status = resp.status();
            if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                if transient_attempt < MAX_TRANSIENT_RETRIES {
                    let backoff = backoff_for(transient_attempt);
                    transient_attempt += 1;
                    tokio::time::sleep(backoff).await;
                    continue;
                }
                anyhow::bail!(
                    "API {} after retries: {}",
                    status,
                    resp.text().await.unwrap_or_default()
                );
            }

            let request_id = resp
                .headers()
                .get("request-id")
                .or_else(|| resp.headers().get("anthropic-request-id"))
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown")
                .to_string();
            let payload: serde_json::Value = resp.json().await?;

            let usage: Usage = serde_json::from_value(payload["usage"].clone()).unwrap_or_default();
            cumulative_usage.add_assign(&usage);
            let text = payload["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .to_string();
            match serde_json::from_str::<Vec<Decision>>(&text) {
                Ok(decisions) => {
                    return Ok(BatchResponse {
                        decisions,
                        usage: cumulative_usage,
                        request_id,
                        wall_ms: started.elapsed().as_millis() as u64,
                    });
                }
                Err(_) if !parse_retried => {
                    parse_retried = true;
                    attempt_blocks.push(ContentBlock {
                        text: PARSE_RETRY_ADDENDUM.to_string(),
                        cache_control: None,
                    });
                    continue;
                }
                Err(e) => {
                    return Err(anyhow::Error::from(ParseFailureError {
                        raw_text: text,
                        request_id,
                        err_msg: e.to_string(),
                    }));
                }
            }
        }
    }
}

fn backoff_for(attempt: usize) -> Duration {
    let base_ms = match attempt {
        0 => 250,
        1 => 1000,
        _ => 1000,
    };
    let jitter = rand::thread_rng().gen_range(0..=base_ms / 2);
    Duration::from_millis(base_ms + jitter)
}

#[derive(Serialize)]
struct ApiRequestBody<'a> {
    model: &'a str,
    temperature: f32,
    max_tokens: u32,
    messages: Vec<UserMessage<'a>>,
}

#[derive(Serialize)]
struct UserMessage<'a> {
    role: &'a str,
    content: Vec<ApiContentBlock<'a>>,
}

#[derive(Serialize)]
struct ApiContentBlock<'a> {
    #[serde(rename = "type")]
    block_type: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl<'a>>,
}

#[derive(Serialize)]
struct CacheControl<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
}

fn build_request_body<'a>(blocks: &'a [ContentBlock]) -> ApiRequestBody<'a> {
    let content: Vec<_> = blocks
        .iter()
        .map(|b| ApiContentBlock {
            block_type: "text",
            text: &b.text,
            cache_control: b.cache_control.map(|k| CacheControl { kind: k }),
        })
        .collect();
    ApiRequestBody {
        model: MODEL_ID,
        temperature: TEMPERATURE,
        max_tokens: MAX_TOKENS,
        messages: vec![UserMessage {
            role: "user",
            content,
        }],
    }
}
