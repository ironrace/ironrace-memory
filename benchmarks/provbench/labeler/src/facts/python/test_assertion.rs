//! Python test-assertion extractor placeholder. Task 10 will populate this
//! with the full `extract` entry point that finds `assert` statements
//! inside `pytest`-style test functions. Task 5 keeps an empty `iter` shim
//! so the module compiles cleanly.

use crate::ast::python::PythonAst;
use crate::ast::spans::Span;

#[allow(dead_code)]
pub fn iter(_ast: &PythonAst) -> impl Iterator<Item = (String, Span)> + '_ {
    std::iter::empty()
}
