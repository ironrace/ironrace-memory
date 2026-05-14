/// SPEC §6.1 verbatim. Byte-equality enforced by `build.rs` and `tests/prompt_frozen.rs`.
/// Edits to this string must be matched by a SPEC §11 entry and a new freeze hash.
pub const PROMPT_TEMPLATE_FROZEN: &str = "\
You are evaluating whether claims about source code are still supported
after a code change.

For each FACT in the FACTS list, decide one of:
  - \"valid\": the change does not affect the fact.
  - \"stale\": the change makes the fact no longer supported.
  - \"needs_revalidation\": the change is relevant but you cannot tell
    from structural information alone whether the fact still holds.

You must base your decision only on the DIFF and the FACT body.
Do not speculate about runtime behavior.

DIFF:
<unified diff, full file context for affected hunks>

FACTS:
<JSON array of {id, kind, body, source_path, line_span, symbol_path,
content_hash_at_observation}>

Respond with a JSON array of {id, decision} only. No prose.";

/// Literal addendum text from SPEC §6.2 for the parse-failure retry. Frozen.
pub const PARSE_RETRY_ADDENDUM: &str =
    "Your previous response was not valid JSON. Respond with a JSON array of {id, decision} only.";

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FactBody {
    pub fact_id: String,
    pub kind: String,
    pub body: String,
    pub source_path: String,
    pub line_span: [u32; 2],
    pub symbol_path: String,
    pub content_hash_at_observation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentBlock {
    pub text: String,
    pub cache_control: Option<&'static str>,
}

pub struct PromptBuilder;

impl PromptBuilder {
    pub fn build(diff: &str, facts: &[FactBody], multi_batch: bool) -> Vec<ContentBlock> {
        let (prefix, suffix) = split_template();
        let block1 = prefix; // includes "DIFF:\n"
        let block2 = diff.to_string();
        let block3 = "\n\nFACTS:\n".to_string();
        let block4 = serde_json::to_string(&fact_payload(facts))
            .expect("FactBody serialization must not fail");
        let block5 = suffix; // trailing instruction line

        vec![
            ContentBlock {
                text: block1,
                cache_control: None,
            },
            ContentBlock {
                text: block2,
                cache_control: None,
            },
            ContentBlock {
                text: block3,
                cache_control: if multi_batch { Some("ephemeral") } else { None },
            },
            ContentBlock {
                text: block4,
                cache_control: None,
            },
            ContentBlock {
                text: block5,
                cache_control: None,
            },
        ]
    }
}

fn split_template() -> (String, String) {
    let tpl = PROMPT_TEMPLATE_FROZEN;
    let diff_marker = "<unified diff, full file context for affected hunks>";
    let facts_marker_start = "<JSON array of {id, kind, body, source_path, line_span, symbol_path,\ncontent_hash_at_observation}>";
    let i_diff = tpl.find(diff_marker).expect("diff placeholder missing");
    let block1 = tpl[..i_diff].to_string();
    let after_diff = i_diff + diff_marker.len();
    let i_facts = tpl[after_diff..]
        .find(facts_marker_start)
        .expect("facts placeholder missing")
        + after_diff;
    let after_facts = i_facts + facts_marker_start.len();
    let block5 = tpl[after_facts..].to_string();
    debug_assert!(block5.contains("Respond with a JSON array"));
    (block1, block5)
}

#[derive(Serialize)]
struct FactPayload<'a> {
    id: &'a str,
    kind: &'a str,
    body: &'a str,
    source_path: &'a str,
    line_span: [u32; 2],
    symbol_path: &'a str,
    content_hash_at_observation: &'a str,
}

fn fact_payload(facts: &[FactBody]) -> Vec<FactPayload<'_>> {
    facts
        .iter()
        .map(|f| FactPayload {
            id: &f.fact_id,
            kind: &f.kind,
            body: &f.body,
            source_path: &f.source_path,
            line_span: f.line_span,
            symbol_path: &f.symbol_path,
            content_hash_at_observation: &f.content_hash_at_observation,
        })
        .collect()
}
