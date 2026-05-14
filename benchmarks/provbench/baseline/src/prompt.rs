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
