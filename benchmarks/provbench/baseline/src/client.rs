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

    pub async fn score_batch(&self, blocks: Vec<ContentBlock>) -> Result<BatchResponse> {
        let started = std::time::Instant::now();
        let mut attempt_blocks = blocks;
        let mut parse_retried = false;

        for transient_attempt in 0..=2 {
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
                Err(e) if transient_attempt < 2 => {
                    let backoff = backoff_for(transient_attempt);
                    tokio::time::sleep(backoff).await;
                    tracing::warn!("transient network error: {e}; retrying after {:?}", backoff);
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            let status = resp.status();
            if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                if transient_attempt < 2 {
                    let backoff = backoff_for(transient_attempt);
                    tokio::time::sleep(backoff).await;
                    continue;
                } else {
                    anyhow::bail!(
                        "API {} after retries: {}",
                        status,
                        resp.text().await.unwrap_or_default()
                    );
                }
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
            let text = payload["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .to_string();
            match serde_json::from_str::<Vec<Decision>>(&text) {
                Ok(decisions) => {
                    return Ok(BatchResponse {
                        decisions,
                        usage,
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
                Err(e) => anyhow::bail!("response parse failed after addendum retry: {e}"),
            }
        }
        anyhow::bail!("score_batch: exhausted retries")
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
