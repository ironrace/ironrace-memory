//! End-to-end parse tests at integration-test scope (separate from inline
//! `#[cfg(test)] mod tests` in llm_reranker.rs).

use ironrace_rerank::{LlmReranker, MockLlmClient, RerankerScorer};

#[test]
fn integration_position_encoding_via_mock() {
    // 5 passages, mock claims rank order [3, 0, 4, 1, 2]: idx 3 best (score 0),
    // idx 0 next (-1), idx 4 (-2), idx 1 (-3), idx 2 worst (-4).
    let client = MockLlmClient::ok("[3, 0, 4, 1, 2]");
    let r = LlmReranker::new(client);
    let scores = r.score_pairs("q", &["a", "b", "c", "d", "e"]).unwrap();
    let expected = vec![-1.0, -3.0, -4.0, 0.0, -2.0];
    assert_eq!(scores, expected);
}

#[test]
fn integration_claude_envelope_round_trips() {
    let envelope = r#"{"type":"result","result":"[1, 0]"}"#;
    let client = MockLlmClient::ok(envelope);
    let r = LlmReranker::new(client);
    let scores = r.score_pairs("q", &["a", "b"]).unwrap();
    assert_eq!(scores, vec![-1.0, 0.0]);
}
