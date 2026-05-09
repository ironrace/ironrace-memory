use provbench_labeler::ast::{spans::content_hash, RustAst};
use provbench_labeler::facts::field;
use provbench_labeler::facts::function_signature;
use provbench_labeler::facts::Fact;

#[test]
fn struct_fields_each_emit_one_fact() {
    let src = b"pub struct Foo { pub a: u32, b: String }\n";
    let ast = provbench_labeler::ast::RustAst::parse(src).unwrap();
    let facts: Vec<_> = field::extract(&ast, std::path::Path::new("a.rs")).collect();
    assert_eq!(facts.len(), 2);
    let names: Vec<_> = facts
        .iter()
        .map(|f| {
            #[allow(unreachable_patterns)]
            match f {
                Fact::Field {
                    qualified_path,
                    type_text,
                    ..
                } => (qualified_path.clone(), type_text.clone()),
                _ => panic!(),
            }
        })
        .collect();
    assert!(names.contains(&("Foo::a".into(), "u32".into())));
    assert!(names.contains(&("Foo::b".into(), "String".into())));
}

#[test]
fn enum_struct_variant_fields_qualified_with_variant() {
    let src = b"pub enum E { V { x: i32 } }\n";
    let ast = provbench_labeler::ast::RustAst::parse(src).unwrap();
    let facts: Vec<_> = field::extract(&ast, std::path::Path::new("a.rs")).collect();
    assert_eq!(facts.len(), 1);
    #[allow(unreachable_patterns)]
    match &facts[0] {
        Fact::Field { qualified_path, .. } => assert_eq!(qualified_path, "E::V::x"),
        _ => panic!(),
    }
}

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

#[test]
fn signature_includes_visibility_and_attrs() {
    let src = b"#[inline]\npub fn add(a: i32) -> i32 { a }\n";
    let ast = provbench_labeler::ast::RustAst::parse(src).unwrap();
    let facts: Vec<_> = function_signature::extract(&ast, std::path::Path::new("a.rs")).collect();
    assert_eq!(facts.len(), 1);
    #[allow(unreachable_patterns)]
    match &facts[0] {
        Fact::FunctionSignature {
            qualified_name,
            span,
            content_hash,
            source_path,
        } => {
            assert_eq!(qualified_name, "add");
            assert_eq!(source_path, std::path::Path::new("a.rs"));
            let body = &src[span.byte_range.clone()];
            assert!(body.starts_with(b"#[inline]"));
            assert!(content_hash.len() == 64);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn nested_module_qualified_name() {
    let src = b"mod a { mod b { pub fn deep() {} } }\n";
    let ast = provbench_labeler::ast::RustAst::parse(src).unwrap();
    let facts: Vec<_> = function_signature::extract(&ast, std::path::Path::new("lib.rs")).collect();
    assert_eq!(facts.len(), 1);
    #[allow(unreachable_patterns)]
    match &facts[0] {
        Fact::FunctionSignature { qualified_name, .. } => {
            assert_eq!(qualified_name, "a::b::deep");
        }
        _ => panic!(),
    }
}
