use provbench_labeler::ast::{spans::content_hash, RustAst};

#[test]
fn parses_function_and_returns_signature_span() {
    let src = b"fn add(a: i32, b: i32) -> i32 { a + b }\n";
    let ast = RustAst::parse(src).unwrap();
    let fns: Vec<_> = ast.function_signature_spans().collect();
    assert_eq!(fns.len(), 1);
    let (name, span) = &fns[0];
    assert_eq!(name, "add");
    let bytes = &src[span.byte_range.clone()];
    let text = std::str::from_utf8(bytes).unwrap();
    assert!(text.starts_with("fn add"));
    assert!(text.ends_with("-> i32"), "got: {text}");
}

#[test]
fn content_hash_is_stable_for_same_bytes() {
    let h1 = content_hash(b"fn x() {}");
    let h2 = content_hash(b"fn x() {}");
    assert_eq!(h1, h2);
    assert_ne!(h1, content_hash(b"fn y() {}"));
    assert_eq!(h1.len(), 64);
}
