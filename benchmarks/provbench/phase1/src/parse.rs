//! Tree-sitter helpers for R5 (whitespace/comment-only) and R6 (doc claim).

use tree_sitter::{Parser, Tree};

pub struct ParsedFile {
    pub source: Vec<u8>,
    pub tree: Option<Tree>,
    pub kind: FileKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Rust,
    Markdown,
    Other,
}

impl ParsedFile {
    pub fn parse_rust(src: &[u8]) -> Self {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_rust::language()).unwrap();
        let tree = p.parse(src, None);
        Self {
            source: src.to_vec(),
            tree,
            kind: FileKind::Rust,
        }
    }
    pub fn parse_markdown(src: &[u8]) -> Self {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_md::language()).unwrap();
        let tree = p.parse(src, None);
        Self {
            source: src.to_vec(),
            tree,
            kind: FileKind::Markdown,
        }
    }
}

/// Returns true if two Rust spans are token-equivalent ignoring
/// whitespace and comments. Uses tree-sitter to identify comment
/// nodes; everything else is compared as a normalized token stream.
pub fn rust_tokens_equivalent(a: &[u8], b: &[u8]) -> bool {
    let toks_a = rust_token_stream(a);
    let toks_b = rust_token_stream(b);
    toks_a == toks_b
}

fn rust_token_stream(src: &[u8]) -> Vec<String> {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_rust::language()).unwrap();
    let tree = match p.parse(src, None) {
        Some(t) => t,
        None => return vec![],
    };
    let mut out = Vec::new();
    walk(&tree.root_node(), src, &mut out);
    out
}

fn walk(node: &tree_sitter::Node<'_>, src: &[u8], out: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "line_comment" || child.kind() == "block_comment" {
            continue;
        }
        if child.child_count() == 0 {
            // Leaf token — capture utf-8 text trimmed.
            if let Ok(s) = std::str::from_utf8(&src[child.start_byte()..child.end_byte()]) {
                let t = s.trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
            }
        } else {
            walk(&child, src, out);
        }
    }
}
