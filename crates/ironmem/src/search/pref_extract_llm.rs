//! LLM-backed implementation of `ironrace_pref_extract::PreferenceExtractor`.
//!
//! The regex extractor produces fragments like `"upgrade my camera flash"` —
//! useful when the question reuses the user's first-person phrasing, but the
//! LongMemEval preference questions almost always reuse *topic vocabulary*
//! ("photography accessories"), not the user's verbs. The LLM extractor asks
//! a small model to summarize the conversation in topic-noun form, producing
//! synth content that embeds closer to question phrasings.

use std::sync::Arc;
use std::time::Duration;

use ironrace_pref_extract::PreferenceExtractor;
use ironrace_rerank::LlmClient;
use serde_json::Value;

const PROMPT_TEMPLATE: &str = "Below are user turns from a conversation. \
Write a 1-2 sentence summary using the kinds of nouns and topics someone \
would use to ASK A QUESTION about this conversation later. Focus on subject \
matter, products, brands, activities, and concerns mentioned. Use natural \
noun phrases. Output only the summary, no preamble, no numbering.

Conversation:
{TEXT}

Summary:";

/// Extractor that calls a single-shot LLM to summarize the conversation in
/// question-vocabulary form. Failure modes (timeout, non-zero exit) return
/// an empty `Vec`, falling back to "no synth doc for this drawer."
pub struct LlmPreferenceExtractor {
    client: Arc<dyn LlmClient>,
}

impl LlmPreferenceExtractor {
    pub fn new(client: Arc<dyn LlmClient>) -> Self {
        Self { client }
    }
}

impl PreferenceExtractor for LlmPreferenceExtractor {
    fn extract(&self, text: &str) -> Vec<String> {
        let prompt = PROMPT_TEMPLATE.replace("{TEXT}", text);
        let raw = match self.client.call(&prompt) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "pref_extract LLM call failed");
                return Vec::new();
            }
        };
        let assistant_text = extract_assistant_text(&raw).unwrap_or(raw);
        let cleaned = assistant_text.trim();
        if cleaned.is_empty() {
            return Vec::new();
        }
        // Return as a single phrase. `synthesize_doc` joins by `". "`; with one
        // entry the join is a no-op so the synth body is exactly this paragraph.
        vec![cleaned.to_string()]
    }
}

/// Pull the assistant text out of the `claude -p --output-format json` envelope:
///   `{"type": "result", "result": "<assistant text>", ...}`
/// Falls back to `None` when the input isn't that envelope; caller then uses
/// the raw stdout directly (covers the API-style `{"content": [...]}` case
/// loosely too).
fn extract_assistant_text(raw: &str) -> Option<String> {
    let v: Value = serde_json::from_str(raw).ok()?;
    if let Some(Value::String(s)) = v.get("result") {
        return Some(s.clone());
    }
    if let Some(Value::Array(parts)) = v.get("content") {
        let mut out = String::new();
        for p in parts {
            if let Some(Value::String(t)) = p.get("text") {
                out.push_str(t);
            }
        }
        if !out.is_empty() {
            return Some(out);
        }
    }
    None
}

/// Build a `ClaudeCliClient`-backed extractor with the given model and timeout.
/// Convenience constructor for production wiring.
pub fn cli_extractor(model: impl Into<String>, timeout: Duration) -> LlmPreferenceExtractor {
    let client: Arc<dyn LlmClient> =
        Arc::new(ironrace_rerank::ClaudeCliClient::new(model, timeout));
    LlmPreferenceExtractor::new(client)
}

/// Direct HTTP client for the Anthropic Messages API. One in-process HTTPS
/// call per `LlmClient::call` — no subprocess, no MCP fan-out, no shell. Use
/// this when running benchmarks or other batch ingest where the per-call
/// overhead of `claude -p` (claude-code init + nested MCP boot) is
/// prohibitive.
///
/// Auth: reads the API key from `ANTHROPIC_API_KEY` first, then
/// `IRONMEM_ANTHROPIC_API_KEY` as a scoped fallback. Returns an error from
/// `call` (which the extractor treats as "no synth this drawer") when both
/// are unset.
pub struct ApiClient {
    pub model: String,
    pub max_tokens: u32,
    pub timeout: Duration,
}

impl ApiClient {
    pub fn new(model: impl Into<String>, max_tokens: u32, timeout: Duration) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            timeout,
        }
    }
}

impl LlmClient for ApiClient {
    fn call(&self, prompt: &str) -> anyhow::Result<String> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("IRONMEM_ANTHROPIC_API_KEY"))
            .map_err(|_| {
                anyhow::anyhow!(
                    "ANTHROPIC_API_KEY (or IRONMEM_ANTHROPIC_API_KEY) must be set for the api backend"
                )
            })?;

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": [{"role": "user", "content": prompt}],
        });

        let agent = ureq::AgentBuilder::new().timeout(self.timeout).build();

        let resp = agent
            .post("https://api.anthropic.com/v1/messages")
            .set("x-api-key", &key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .send_json(body);

        let body_text = match resp {
            Ok(r) => r.into_string()?,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                anyhow::bail!("anthropic api {code}: {body}");
            }
            Err(e) => anyhow::bail!("anthropic api transport: {e}"),
        };
        Ok(body_text)
    }
}

/// Build an `ApiClient`-backed extractor.
pub fn api_extractor(
    model: impl Into<String>,
    max_tokens: u32,
    timeout: Duration,
) -> LlmPreferenceExtractor {
    let client: Arc<dyn LlmClient> = Arc::new(ApiClient::new(model, max_tokens, timeout));
    LlmPreferenceExtractor::new(client)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ironrace_rerank::MockLlmClient;

    #[test]
    fn extracts_summary_from_envelope_response() {
        let mock = MockLlmClient::ok(
            r#"{"type":"result","result":"User discussed Sony A7R IV photography setup, Godox V1 flash, Gitzo tripod, and camera bag accessories."}"#,
        );
        let ex = LlmPreferenceExtractor::new(Arc::new(mock));
        let phrases = ex.extract("I'm upgrading my Sony A7R IV...");
        assert_eq!(phrases.len(), 1);
        assert!(phrases[0].contains("photography"));
        assert!(phrases[0].contains("Godox"));
    }

    #[test]
    fn extracts_summary_from_raw_text_when_not_json() {
        let mock = MockLlmClient::ok(
            "User discussed kitchen organization, slow cooker recipes, and meal prep ideas.",
        );
        let ex = LlmPreferenceExtractor::new(Arc::new(mock));
        let phrases = ex.extract("I've been cooking...");
        assert_eq!(phrases.len(), 1);
        assert!(phrases[0].contains("slow cooker"));
    }

    #[test]
    fn returns_empty_on_llm_error() {
        let mock = MockLlmClient::err("subprocess failed");
        let ex = LlmPreferenceExtractor::new(Arc::new(mock));
        assert!(ex.extract("anything").is_empty());
    }

    #[test]
    fn returns_empty_on_blank_response() {
        let mock = MockLlmClient::ok(r#"{"type":"result","result":"   "}"#);
        let ex = LlmPreferenceExtractor::new(Arc::new(mock));
        assert!(ex.extract("anything").is_empty());
    }
}
