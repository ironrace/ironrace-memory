//! Integration tests for `resolve::python::PythonResolver` (Task 11).
//!
//! Walks the canonical fixture under `tests/data/python/repo/` and asserts
//! that module-level bindings, class members, and import edges are
//! resolved deterministically across two independent index runs.

mod common;

use provbench_labeler::resolve::{python::PythonResolver, SymbolResolver};

#[test]
fn resolves_module_function() {
    let mut r = PythonResolver::index(common::python_fixture_repo()).unwrap();
    let loc = r
        .resolve("src.example.async_op")
        .unwrap()
        .expect("async_op should resolve");
    assert!(
        loc.file.ends_with("src/example.py"),
        "unexpected file: {:?}",
        loc.file
    );
}

#[test]
fn resolves_class() {
    let mut r = PythonResolver::index(common::python_fixture_repo()).unwrap();
    let loc = r
        .resolve("src.example.Greeter")
        .unwrap()
        .expect("Greeter should resolve");
    assert!(loc.file.ends_with("src/example.py"));
}

#[test]
fn resolves_class_method() {
    let mut r = PythonResolver::index(common::python_fixture_repo()).unwrap();
    let loc = r
        .resolve("src.example.Greeter.greet")
        .unwrap()
        .expect("greet should resolve");
    assert!(loc.file.ends_with("src/example.py"));
}

#[test]
fn resolves_through_import() {
    // tests/test_example.py has `from src.example import Greeter`
    // so `tests.test_example.Greeter` should resolve to src/example.py.
    let mut r = PythonResolver::index(common::python_fixture_repo()).unwrap();
    let loc = r
        .resolve("tests.test_example.Greeter")
        .unwrap()
        .expect("imported Greeter should resolve");
    assert!(
        loc.file.ends_with("src/example.py"),
        "should follow import edge to src/example.py, got {:?}",
        loc.file
    );
}

#[test]
fn unresolved_returns_none() {
    let mut r = PythonResolver::index(common::python_fixture_repo()).unwrap();
    assert!(r.resolve("src.example.does_not_exist").unwrap().is_none());
}

#[test]
fn determinism_two_indexes_produce_same_resolutions() {
    let mut r1 = PythonResolver::index(common::python_fixture_repo()).unwrap();
    let mut r2 = PythonResolver::index(common::python_fixture_repo()).unwrap();
    for name in [
        "src.example.async_op",
        "src.example.Greeter",
        "src.example.Greeter.greet",
        "tests.test_example.Greeter",
    ] {
        assert_eq!(
            r1.resolve(name).unwrap(),
            r2.resolve(name).unwrap(),
            "resolution differs across two index runs for {name}"
        );
    }
}
