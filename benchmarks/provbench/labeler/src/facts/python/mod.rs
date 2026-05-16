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

use std::path::Path;

/// Compute a module path for a Python source file. Strips the trailing
/// `.py` extension and replaces path separators with `.`.
///
/// **Note:** the path is preserved verbatim — a file at
/// `src/example.py` becomes `src.example`. Task 11 (PythonResolver) may
/// refine this when stripping repo-root prefixes / collapsing
/// `__init__.py`; until then the fixture's expected qualified names
/// (e.g. `src.example.Greeter.greet`) drive the policy here.
pub(super) fn module_path_for(source_path: &Path) -> String {
    let s = source_path.to_string_lossy();
    let stripped = s.strip_suffix(".py").unwrap_or(&s);
    stripped.replace('/', ".")
}
