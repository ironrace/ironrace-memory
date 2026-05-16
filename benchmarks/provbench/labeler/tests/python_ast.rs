use provbench_labeler::ast::python::PythonAst;

const SRC: &str = include_str!("data/python/repo/src/example.py");

#[test]
fn parse_succeeds() {
    PythonAst::parse(SRC.as_bytes()).unwrap();
}

#[test]
fn function_signature_spans_lists_all_defs() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let mut sigs: Vec<_> = ast.function_signature_spans().collect();
    sigs.sort_by(|a, b| a.0.cmp(&b.0));
    let names: Vec<&str> = sigs.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["_private", "async_op", "greet"]);
}

#[test]
fn class_spans_lists_classes() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let classes: Vec<_> = ast.class_spans().collect();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].0, "Greeter");
}

#[test]
fn module_constant_spans_lists_uppercase_bindings() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let consts: Vec<_> = ast.module_constant_spans().collect();
    let names: Vec<&str> = consts.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["CONSTANT_X"]);
}

#[test]
fn signature_span_stops_before_body() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let (name, span) = ast
        .function_signature_spans()
        .find(|(n, _)| n == "greet")
        .unwrap();
    let signature_text = std::str::from_utf8(&SRC.as_bytes()[span.byte_range.clone()]).unwrap();
    assert!(signature_text.starts_with("def greet"));
    assert!(signature_text.ends_with("-> str:") || signature_text.ends_with("-> str"));
    assert!(!signature_text.contains("return"));
    assert_eq!(name, "greet");
}
