//! Python DocClaim extractor — DEFERRED.
//!
//! The Rust analog (`crate::facts::doc_claim::extract`) takes `known_facts`
//! and back-resolves inline-code mentions in `.md` files to their defining
//! Fact's span+hash. Wiring the Python labeler into that pipeline requires:
//!  - Task 11 PythonResolver to be in place so cross-file Python symbol
//!    mentions can be resolved.
//!  - Task 12 replay integration to pass the merged `known_facts` set
//!    (Rust + Python facts) into the markdown scanner.
//!  - Extending `find_mentions` to match the full dotted form of a Python
//!    qualified_name (`flask.app.Flask.run`), not just the last segment
//!    (`run`), so Python-style refs in `.md` files are surfaced.
//!
//! For v1.2b held-out evaluation, Python `DocClaim` facts will be empty
//! and R5 (`stale_doc_drift`) will not fire on Python rows. This is a
//! recorded hygiene limitation and surfaces in the flask findings doc.
//!
//! A no-op `extract` is provided so the dispatch sites in Task 12 can
//! call the same shape for both languages.

use crate::ast::python::PythonAst;
use crate::facts::Fact;
use std::path::Path;

pub fn extract<'a>(
    _ast: &'a PythonAst,
    _source_path: &'a Path,
) -> impl Iterator<Item = Fact> + 'a {
    std::iter::empty()
}
