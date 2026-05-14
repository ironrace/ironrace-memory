//! Build-time SPEC §6.1 prompt drift check.

use std::path::PathBuf;

fn main() {
    let spec_path: PathBuf = PathBuf::from("../SPEC.md");
    println!("cargo:rerun-if-changed=../SPEC.md");
    println!("cargo:rerun-if-changed=src/prompt.rs");
    let spec = std::fs::read_to_string(&spec_path)
        .unwrap_or_else(|e| panic!("read SPEC.md: {e} (cwd-sensitive build)"));
    let block = extract_block(&spec);

    let prompt_rs = std::fs::read_to_string("src/prompt.rs").unwrap();
    let frozen = extract_frozen_const(&prompt_rs);

    if block.trim_end_matches('\n') != frozen.trim_end_matches('\n') {
        panic!(
            "SPEC §6.1 drifted from src/prompt.rs::PROMPT_TEMPLATE_FROZEN\n\
             SPEC block:\n{}\n---\nCONST:\n{}\n",
            block, frozen
        );
    }
}

fn extract_block(spec: &str) -> String {
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

fn extract_frozen_const(rs: &str) -> String {
    let start = rs.find("PROMPT_TEMPLATE_FROZEN").expect("const missing");
    let after_eq = rs[start..]
        .find("= \"\\\n")
        .expect("expected `= \"\\\n` opening");
    let body_start = start + after_eq + "= \"\\\n".len();
    let body_end = rs[body_start..].find("\";").expect("closing `\";` missing") + body_start;
    rs[body_start..body_end]
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}
