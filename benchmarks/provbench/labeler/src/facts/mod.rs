//! Closed enum of fact kinds (SPEC §3.1). Adding a kind is a §11 spec
//! change — do not extend silently.

pub mod doc_claim;
pub mod field;
pub mod function_signature;
pub mod symbol_existence;

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
}
