use provbench_baseline::prompt::{FactBody, PromptBuilder};

fn one_fact() -> Vec<FactBody> {
    vec![FactBody {
        fact_id: "X".into(),
        kind: "FunctionSignature".into(),
        body: "x".into(),
        source_path: "x".into(),
        line_span: [1, 1],
        symbol_path: "x".into(),
        content_hash_at_observation: "0".repeat(64),
    }]
}

#[test]
fn cache_control_only_on_block_3_when_multi_batch() {
    let blocks = PromptBuilder::build("D", &one_fact(), true);
    assert!(blocks[0].cache_control.is_none());
    assert!(blocks[1].cache_control.is_none());
    assert!(
        blocks[2].cache_control.is_some(),
        "block 3 caches when multi_batch=true"
    );
    assert_eq!(*blocks[2].cache_control.as_ref().unwrap(), "ephemeral");
    assert!(
        blocks[3].cache_control.is_none(),
        "FACTS must never be cached"
    );
    assert!(blocks[4].cache_control.is_none());
}

#[test]
fn no_cache_control_on_single_batch() {
    let blocks = PromptBuilder::build("D", &one_fact(), false);
    for (i, b) in blocks.iter().enumerate() {
        assert!(
            b.cache_control.is_none(),
            "block {i} must have no cache_control on single-batch"
        );
    }
}
