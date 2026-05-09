//! Tree-sitter Rust parser wrapper. Owns the parser handle and tree;
//! offers high-level iterators per fact kind. Python support will be
//! added in a sibling module — keep this Rust-only.

pub mod spans;

use anyhow::{Context, Result};
use spans::Span;
use tree_sitter::{Node, Parser, Tree};

pub struct RustAst {
    src: Vec<u8>,
    tree: Tree,
}

impl RustAst {
    pub fn parse(src: &[u8]) -> Result<Self> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .context("set rust language")?;
        let tree = parser
            .parse(src, None)
            .ok_or_else(|| anyhow::anyhow!("tree-sitter returned no tree"))?;
        Ok(Self {
            src: src.to_vec(),
            tree,
        })
    }

    pub fn source(&self) -> &[u8] {
        &self.src
    }

    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }

    /// Yield (function name, signature span) pairs. The signature span
    /// covers `fn NAME(...) -> R` and stops before the function body.
    pub fn function_signature_spans(&self) -> impl Iterator<Item = (String, Span)> + '_ {
        crate::facts::function_signature::iter(self)
    }
}

#[allow(dead_code)]
pub(crate) fn line_span_from_node(_src: &[u8], node: Node<'_>) -> Span {
    Span {
        byte_range: node.start_byte()..node.end_byte(),
        line_start: (node.start_position().row + 1) as u32,
        line_end: (node.end_position().row + 1) as u32,
    }
}

#[allow(dead_code)]
pub(crate) fn line_span_through(src: &[u8], start: Node<'_>, end_byte: usize) -> Span {
    let line_end_row = src[..end_byte].iter().filter(|b| **b == b'\n').count() as u32 + 1;
    Span {
        byte_range: start.start_byte()..end_byte,
        line_start: (start.start_position().row + 1) as u32,
        line_end: line_end_row,
    }
}
