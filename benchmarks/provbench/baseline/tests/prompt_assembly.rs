use provbench_baseline::prompt::{ContentBlock, FactBody, PromptBuilder};

#[test]
fn five_blocks_in_order_with_correct_static_text() {
    let facts = vec![FactBody {
        fact_id: "FunctionSignature::foo".into(),
        kind: "FunctionSignature".into(),
        body: "function foo has parameters () with return type ()".into(),
        source_path: "src/lib.rs".into(),
        line_span: [1, 3],
        symbol_path: "foo".into(),
        content_hash_at_observation: "0".repeat(64),
    }];
    let blocks: Vec<ContentBlock> = PromptBuilder::build("--- a/x\n+++ b/x", &facts, false);
    assert_eq!(blocks.len(), 5, "must be exactly 5 blocks");
    assert!(
        blocks[0].text.ends_with("DIFF:\n"),
        "block 1 ends with DIFF:\\n"
    );
    assert_eq!(blocks[1].text, "--- a/x\n+++ b/x");
    assert_eq!(blocks[2].text, "\n\nFACTS:\n");
    assert!(blocks[3].text.starts_with('['), "block 4 is a JSON array");
    assert!(blocks[3].text.contains("FunctionSignature::foo"));
    assert!(blocks[4].text.contains("Respond with a JSON array"));
    assert!(
        blocks[4].text.starts_with("\n\nRespond"),
        "block 5 must preserve the blank-line separator after FACTS JSON"
    );

    let rendered = blocks.iter().map(|b| b.text.as_str()).collect::<String>();
    assert!(
        rendered.contains("}]\n\nRespond with a JSON array"),
        "FACTS JSON must be separated from the final instruction"
    );
}
