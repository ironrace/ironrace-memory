//! Python per-fact-kind extractors. Mirrors the Rust extractor tree in
//! [`crate::facts`], but operates on a [`crate::ast::python::PythonAst`].
//! Task 5 only ships the AST-walking iterators that back
//! [`PythonAst::function_signature_spans`], [`PythonAst::class_spans`], and
//! [`PythonAst::module_constant_spans`]; the full `extract` / [`Fact`]
//! emission layer lands in Tasks 6-10.

pub mod doc_claim;
pub mod field;
pub mod function_signature;
pub mod symbol_existence;
pub mod test_assertion;
