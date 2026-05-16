//! Closed enum of fact kinds (SPEC §3.1). Adding a kind is a §11 spec
//! change — do not extend silently.
//!
//! The five fact kinds — function signatures, struct/enum fields, public
//! symbols, doc claims, and test assertions — are extracted at T₀ across
//! the pilot tree by per-kind submodules. Each submodule exposes an
//! `extract` entry point that takes a parsed [`crate::ast::RustAst`] (or
//! markdown bytes for [`doc_claim`]) and yields the corresponding
//! [`Fact`] variant.

pub mod doc_claim;
pub mod field;
pub mod function_signature;
pub mod python;
pub mod symbol_existence;
pub mod test_assertion;

use crate::ast::spans::Span;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Fact {
    FunctionSignature {
        qualified_name: String,
        source_path: PathBuf,
        span: Span,
        content_hash: String,
    },
    Field {
        qualified_path: String,
        source_path: PathBuf,
        type_text: String,
        span: Span,
        content_hash: String,
    },
    PublicSymbol {
        qualified_name: String,
        source_path: PathBuf,
        span: Span,
        content_hash: String,
    },
    DocClaim {
        /// Last-segment name of the resolved fact (e.g. `"search"`).
        qualified_name: String,
        /// Path to the markdown file that contains the mention.
        doc_path: PathBuf,
        /// Byte span of the inline-code mention inside the markdown file.
        mention_span: Span,
        /// SHA-256 of the bytes in `mention_span`.
        mention_hash: String,
        /// Byte span of the defining fact (copied from the resolved Fact).
        defining_span: Span,
        /// Content hash of the defining fact (copied from the resolved Fact).
        defining_hash: String,
    },
    TestAssertion {
        /// Name of the `#[test]`-annotated function containing the assertion.
        test_fn: String,
        /// Source file the assertion lives in.
        source_path: PathBuf,
        /// Byte span of the `assert!` / `assert_eq!` / `assert_ne!` macro
        /// invocation (including the macro name and argument list).
        span: Span,
        /// SHA-256 of the bytes in `span`.
        content_hash: String,
        /// Last-segment name of the first identifier in the assertion
        /// arguments that matches a known fact's qualified name. `None` when
        /// no such match is found.
        asserted_symbol: Option<String>,
    },
}

impl Fact {
    /// SPEC §3 kind name used as the `kind` discriminator in
    /// `FactBodyRow` and as the leading segment of `fact_id`.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Fact::FunctionSignature { .. } => "FunctionSignature",
            Fact::Field { .. } => "Field",
            Fact::PublicSymbol { .. } => "PublicSymbol",
            Fact::DocClaim { .. } => "DocClaim",
            Fact::TestAssertion { .. } => "TestAssertion",
        }
    }

    /// Source file path the fact is bound to. For `DocClaim` this is the
    /// markdown file; for every other kind it is the `.rs` file.
    pub fn source_path(&self) -> &std::path::Path {
        match self {
            Fact::FunctionSignature { source_path, .. } => source_path,
            Fact::Field { source_path, .. } => source_path,
            Fact::PublicSymbol { source_path, .. } => source_path,
            Fact::DocClaim { doc_path, .. } => doc_path,
            Fact::TestAssertion { source_path, .. } => source_path,
        }
    }

    /// `[line_start, line_end]` (1-based, inclusive) for the fact's
    /// bound span — packaged in the order [`crate::output::FactBodyRow`]
    /// expects.
    pub fn line_span(&self) -> [u32; 2] {
        let span = match self {
            Fact::FunctionSignature { span, .. } => span,
            Fact::Field { span, .. } => span,
            Fact::PublicSymbol { span, .. } => span,
            Fact::DocClaim { mention_span, .. } => mention_span,
            Fact::TestAssertion { span, .. } => span,
        };
        [span.line_start, span.line_end]
    }

    /// Stable identifier of the fact's bound symbol. Used as the
    /// `symbol_path` field in `FactBodyRow`. The exact shape varies per
    /// kind (`qualified_name`, `qualified_path`, or `test_fn`) and
    /// mirrors what the SPEC §5 rule engine uses as the primary key for
    /// T₀ → post pairing.
    pub fn symbol_path(&self) -> String {
        match self {
            Fact::FunctionSignature { qualified_name, .. } => qualified_name.clone(),
            Fact::Field { qualified_path, .. } => qualified_path.clone(),
            Fact::PublicSymbol { qualified_name, .. } => qualified_name.clone(),
            Fact::DocClaim { qualified_name, .. } => qualified_name.clone(),
            Fact::TestAssertion { test_fn, .. } => test_fn.clone(),
        }
    }

    /// 64-char lowercase hex SHA-256 of the fact's bound span at T₀.
    /// For `DocClaim` this is `mention_hash` (the inline-code mention),
    /// matching how SPEC §5 step 3 hashes the bound span.
    pub fn content_hash(&self) -> &str {
        match self {
            Fact::FunctionSignature { content_hash, .. } => content_hash,
            Fact::Field { content_hash, .. } => content_hash,
            Fact::PublicSymbol { content_hash, .. } => content_hash,
            Fact::DocClaim { mention_hash, .. } => mention_hash,
            Fact::TestAssertion { content_hash, .. } => content_hash,
        }
    }
}
