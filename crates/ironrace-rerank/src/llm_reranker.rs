//! LLM-based reranker. Implements `RerankerScorer` by calling out to an
//! `LlmClient` (production: Claude Haiku via `claude` CLI).

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::llm_client::LlmClient;
use crate::scorer::RerankerScorer;

/// Pinned rerank prompt template. Changes require a full eval re-run.
const REREANK_PROMPT_TEMPLATE: &str = "You are a relevance ranker. Given a search query and N candidate passages, return the candidates ranked from most to least relevant to the query.\n\nQuery: {QUERY}\n\nCandidates:\n{CANDIDATES}\n\nOutput: a JSON array of candidate indices ordered from most to least relevant.\nOutput ONLY the JSON array, no commentary.\nExample output for N=4: [2, 0, 3, 1]\n";

/// Truncate each passage to this many chars before sending. Mempalace's choice.
const PASSAGE_MAX_CHARS: usize = 500;

pub struct LlmReranker<C: LlmClient> {
    client: C,
}

impl<C: LlmClient> LlmReranker<C> {
    pub fn new(client: C) -> Self {
        Self { client }
    }
}

impl<C: LlmClient> RerankerScorer for LlmReranker<C> {
    fn score_pairs(&self, query: &str, passages: &[&str]) -> Result<Vec<f32>> {
        if passages.is_empty() {
            return Ok(Vec::new());
        }
        let prompt = build_rerank_prompt(query, passages);
        let raw = self
            .client
            .call(&prompt)
            .context("LLM client call failed")?;
        let order = parse_rerank_response(&raw, passages.len())?;
        // Encode rerank position as score: position 0 (most relevant) → 0.0,
        // position 1 → -1.0, ... missing indices → -infinity (sink).
        let mut scores = vec![f32::NEG_INFINITY; passages.len()];
        for (rank, &idx) in order.iter().enumerate() {
            if idx < scores.len() {
                scores[idx] = -(rank as f32);
            }
        }
        Ok(scores)
    }
}

fn truncate_chars(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    // Find a UTF-8-safe boundary at or before `max`.
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn build_rerank_prompt(query: &str, passages: &[&str]) -> String {
    let mut candidates = String::new();
    for (i, p) in passages.iter().enumerate() {
        candidates.push_str(&format!("[{i}] {}\n", truncate_chars(p, PASSAGE_MAX_CHARS)));
    }
    REREANK_PROMPT_TEMPLATE
        .replace("{QUERY}", query)
        .replace("{CANDIDATES}", &candidates)
}

/// Parse the LLM's response into a list of candidate indices, most-to-least
/// relevant. Tolerant: try strict JSON envelope first, then bracket extraction
/// from prose, then bail.
fn parse_rerank_response(raw: &str, expected_n: usize) -> Result<Vec<usize>> {
    // Try: claude --output-format json envelope. Schema:
    //   { "type": "result", "result": "<assistant text>", ... }
    // The assistant text is what we care about.
    let assistant_text = extract_assistant_text(raw).unwrap_or_else(|| raw.to_string());

    // Try parsing assistant_text as a JSON array first.
    if let Ok(arr) = serde_json::from_str::<Vec<usize>>(assistant_text.trim()) {
        return validate_order(arr, expected_n);
    }

    // Fallback: scan for the first `[ ... ]` containing comma-separated ints.
    if let Some(arr) = extract_bracketed_ints(&assistant_text) {
        return validate_order(arr, expected_n);
    }

    tracing::trace!(raw = %raw, assistant = %assistant_text, "rerank response parse failed");
    bail!("could not parse a JSON array of {expected_n} indices from LLM response")
}

fn extract_assistant_text(raw: &str) -> Option<String> {
    let v: Value = serde_json::from_str(raw).ok()?;
    // Common shapes:
    //   { "result": "<text>" }                          ← claude -p --output-format json
    //   { "type": "result", "result": "<text>" }
    if let Some(Value::String(s)) = v.get("result") {
        return Some(s.clone());
    }
    None
}

fn extract_bracketed_ints(s: &str) -> Option<Vec<usize>> {
    let start = s.find('[')?;
    let end = s[start..].find(']')? + start;
    let inner = &s[start + 1..end];
    let mut out = Vec::new();
    for tok in inner.split(',') {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        out.push(t.parse::<usize>().ok()?);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn validate_order(order: Vec<usize>, expected_n: usize) -> Result<Vec<usize>> {
    if order.is_empty() {
        bail!("empty order list");
    }
    // Drop any out-of-range indices but keep the rest. Don't fail hard — the
    // pipeline will assign NEG_INFINITY to anything missing, which is the
    // graceful-degradation behavior we want.
    Ok(order.into_iter().filter(|&i| i < expected_n).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_client::MockLlmClient;

    #[test]
    fn parse_strict_json_array() {
        let order = parse_rerank_response("[2, 0, 1]", 3).unwrap();
        assert_eq!(order, vec![2, 0, 1]);
    }

    #[test]
    fn parse_claude_envelope() {
        let raw = r#"{"type":"result","result":"[1, 0, 2]"}"#;
        let order = parse_rerank_response(raw, 3).unwrap();
        assert_eq!(order, vec![1, 0, 2]);
    }

    #[test]
    fn parse_prose_with_array() {
        let raw = "Sure, here's the ranking: [3, 0, 2, 1] — that should help.";
        let order = parse_rerank_response(raw, 4).unwrap();
        assert_eq!(order, vec![3, 0, 2, 1]);
    }

    #[test]
    fn parse_drops_out_of_range_indices() {
        let order = parse_rerank_response("[2, 99, 0, 1]", 3).unwrap();
        assert_eq!(order, vec![2, 0, 1]);
    }

    #[test]
    fn parse_malformed_errors() {
        assert!(parse_rerank_response("not even close", 3).is_err());
    }

    #[test]
    fn score_pairs_position_encoded() {
        // Mock returns [2, 0, 1]: idx 2 gets score 0, idx 0 gets -1, idx 1 gets -2.
        let client = MockLlmClient::ok("[2, 0, 1]");
        let r = LlmReranker::new(client);
        let scores = r.score_pairs("q", &["a", "b", "c"]).unwrap();
        assert_eq!(scores, vec![-1.0, -2.0, 0.0]);
    }

    #[test]
    fn score_pairs_missing_index_sinks() {
        // Mock returns [2, 0] — idx 1 missing.
        let client = MockLlmClient::ok("[2, 0]");
        let r = LlmReranker::new(client);
        let scores = r.score_pairs("q", &["a", "b", "c"]).unwrap();
        assert_eq!(scores[0], -1.0);
        assert_eq!(scores[1], f32::NEG_INFINITY);
        assert_eq!(scores[2], 0.0);
    }

    #[test]
    fn score_pairs_empty() {
        let client = MockLlmClient::ok("[]");
        let r = LlmReranker::new(client);
        assert!(r.score_pairs("q", &[]).unwrap().is_empty());
    }

    #[test]
    fn score_pairs_client_error_propagates() {
        let client = MockLlmClient::err("network down");
        let r = LlmReranker::new(client);
        assert!(r.score_pairs("q", &["a"]).is_err());
    }
}
