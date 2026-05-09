//! Mechanical labeling rule engine. SPEC §5 first-match-wins ordering.
//!
//! The rule order is the contract — never reorder without a §11 entry.

use crate::facts::Fact;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Label {
    Valid,
    StaleSourceChanged,
    StaleSourceDeleted,
    StaleSymbolRenamed { new_name: String },
    NeedsRevalidation,
}

pub trait PostCommitState {
    fn file_exists(&self) -> bool;
    fn symbol_resolves(&self) -> bool;
    fn rename_candidate(&self) -> Option<&str>;
    fn post_span_hash(&self) -> Option<&str>;
    fn whitespace_or_comment_only(&self) -> bool;
    fn structurally_classifiable(&self) -> bool;
}

pub fn classify(fact: &Fact, state: &dyn PostCommitState) -> Label {
    if !state.file_exists() {
        return Label::StaleSourceDeleted;
    }
    if !state.symbol_resolves() {
        return match state.rename_candidate() {
            Some(new_name) => Label::StaleSymbolRenamed {
                new_name: new_name.to_string(),
            },
            None => Label::StaleSourceDeleted,
        };
    }
    let observed_hash = fact_hash(fact);
    if let Some(post) = state.post_span_hash() {
        if post == observed_hash {
            return Label::Valid;
        }
        if state.whitespace_or_comment_only() {
            return Label::Valid;
        }
        if state.structurally_classifiable() {
            return Label::StaleSourceChanged;
        }
        return Label::NeedsRevalidation;
    }
    Label::NeedsRevalidation
}

fn fact_hash(fact: &Fact) -> &str {
    match fact {
        Fact::FunctionSignature { content_hash, .. }
        | Fact::Field { content_hash, .. }
        | Fact::PublicSymbol { content_hash, .. }
        | Fact::TestAssertion { content_hash, .. } => content_hash,
        Fact::DocClaim { mention_hash, .. } => mention_hash,
    }
}

#[derive(Debug, Default)]
pub struct MockState {
    pub file_exists: bool,
    pub symbol_resolves: bool,
    pub rename_candidate: Option<String>,
    pub post_span_hash: Option<String>,
    pub whitespace_or_comment_only: bool,
    pub structurally_classifiable: bool,
}

impl PostCommitState for MockState {
    fn file_exists(&self) -> bool {
        self.file_exists
    }
    fn symbol_resolves(&self) -> bool {
        self.symbol_resolves
    }
    fn rename_candidate(&self) -> Option<&str> {
        self.rename_candidate.as_deref()
    }
    fn post_span_hash(&self) -> Option<&str> {
        self.post_span_hash.as_deref()
    }
    fn whitespace_or_comment_only(&self) -> bool {
        self.whitespace_or_comment_only
    }
    fn structurally_classifiable(&self) -> bool {
        self.structurally_classifiable
    }
}
