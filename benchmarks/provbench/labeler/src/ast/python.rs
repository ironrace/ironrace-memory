//! Tree-sitter Python parser wrapper. Mirrors `RustAst` in module shape:
//! `parse`, `source`, `root`, plus per-fact-kind span iterators.
//!
//! Span definitions:
//!  - function signature: from `def`/`async def` keyword through the `:`
//!    that opens the body (exclusive of the body block).
//!  - class: from the `class` keyword through the trailing `:` of the header.
//!  - module constant: assignment statements at module scope whose LHS is
//!    a single Name in SCREAMING_SNAKE_CASE (`[A-Z][A-Z0-9_]*`).

use anyhow::{Context, Result};
use tree_sitter::{Node, Parser, Tree};

use super::spans::Span;

pub struct PythonAst {
    src: Vec<u8>,
    tree: Tree,
}

impl PythonAst {
    pub fn parse(src: &[u8]) -> Result<Self> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .context("set python language")?;
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

    pub fn function_signature_spans(&self) -> impl Iterator<Item = (String, Span)> + '_ {
        crate::facts::python::function_signature::iter(self)
    }

    pub fn class_spans(&self) -> impl Iterator<Item = (String, Span)> + '_ {
        crate::facts::python::field::iter_classes(self)
    }

    pub fn module_constant_spans(&self) -> impl Iterator<Item = (String, Span)> + '_ {
        crate::facts::python::symbol_existence::iter_module_constants(self)
    }
}
