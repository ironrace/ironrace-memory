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
