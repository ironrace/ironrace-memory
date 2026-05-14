//! Byte-equality between SPEC §6.1 fenced prompt block and PROMPT_TEMPLATE_FROZEN.

use provbench_baseline::prompt::PROMPT_TEMPLATE_FROZEN;

#[test]
fn frozen_prompt_matches_spec_byte_for_byte() {
    let spec = include_str!("../../SPEC.md");
    let block = extract_section_6_1_block(spec);
    assert_eq!(
        block.trim_end_matches('\n'),
        PROMPT_TEMPLATE_FROZEN.trim_end_matches('\n'),
        "SPEC §6.1 prompt block drifted from PROMPT_TEMPLATE_FROZEN — \
         spec change requires §11 entry and a new freeze hash"
    );
}

fn extract_section_6_1_block(spec: &str) -> String {
    let mut lines = spec.lines();
    for line in lines.by_ref() {
        if line.starts_with("### 6.1 Prompt") {
            break;
        }
    }
    for line in lines.by_ref() {
        if line.trim() == "```" {
            break;
        }
    }
    let mut out = String::new();
    for line in lines {
        if line.trim() == "```" {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}
