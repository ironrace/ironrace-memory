//! Closed enum of fact kinds (SPEC §3.1). Adding a kind is a §11 spec
//! change — do not extend silently.

pub mod function_signature;

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
}
