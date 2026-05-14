use provbench_baseline::client::AnthropicClient;
use provbench_baseline::prompt::{ContentBlock, FactBody, PromptBuilder};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn one_block_request() -> Vec<ContentBlock> {
    let facts = vec![FactBody {
        fact_id: "X".into(),
        kind: "FunctionSignature".into(),
        body: "b".into(),
        source_path: "x".into(),
        line_span: [1, 1],
        symbol_path: "x".into(),
        content_hash_at_observation: "0".repeat(64),
    }];
    PromptBuilder::build("D", &facts, false)
}

#[tokio::test]
async fn retries_on_5xx_then_succeeds() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"[{\"id\":\"X\",\"decision\":\"valid\"}]"}],"usage":{"input_tokens":10,"output_tokens":5}}"#,
        ))
        .mount(&mock)
        .await;

    let client = AnthropicClient::with_base_url(mock.uri(), "test-key".into());
    let resp = client.score_batch(one_block_request()).await.unwrap();
    assert_eq!(resp.decisions.len(), 1);
}

#[tokio::test]
async fn parse_error_triggers_one_retry_with_literal_addendum() {
    use provbench_baseline::prompt::PARSE_RETRY_ADDENDUM;
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"I cannot do this."}],"usage":{"input_tokens":10,"output_tokens":5}}"#,
        ))
        .up_to_n_times(1)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"id":"msg_2","type":"message","role":"assistant","content":[{"type":"text","text":"[{\"id\":\"X\",\"decision\":\"stale\"}]"}],"usage":{"input_tokens":15,"output_tokens":7}}"#,
        ))
        .mount(&mock)
        .await;

    let client = AnthropicClient::with_base_url(mock.uri(), "k".into());
    let resp = client.score_batch(one_block_request()).await.unwrap();
    assert_eq!(resp.decisions[0].decision, "stale");

    let requests = mock.received_requests().await.unwrap();
    let second_body = std::str::from_utf8(&requests[1].body).unwrap();
    assert!(second_body.contains(PARSE_RETRY_ADDENDUM));
}
