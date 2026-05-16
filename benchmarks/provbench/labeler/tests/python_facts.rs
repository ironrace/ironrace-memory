use provbench_labeler::ast::python::PythonAst;
use provbench_labeler::facts::python::field;
use provbench_labeler::facts::python::function_signature;
use provbench_labeler::facts::Fact;
use std::path::Path;

const SRC: &str = include_str!("data/python/repo/src/example.py");

fn qualified_names(facts: &[Fact]) -> Vec<String> {
    let mut names: Vec<String> = facts
        .iter()
        .filter_map(|f| match f {
            Fact::FunctionSignature { qualified_name, .. } => Some(qualified_name.clone()),
            _ => None,
        })
        .collect();
    names.sort();
    names
}

#[test]
fn function_signature_emits_one_fact_per_def() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let facts: Vec<Fact> = function_signature::extract(&ast, Path::new("src/example.py")).collect();
    assert_eq!(
        qualified_names(&facts),
        vec![
            "src.example.Greeter.greet".to_string(),
            "src.example._private".to_string(),
            "src.example.async_op".to_string(),
        ]
    );
}

#[test]
fn function_signature_content_hash_is_signature_only() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let facts: Vec<Fact> = function_signature::extract(&ast, Path::new("src/example.py")).collect();
    let greet_hash = facts
        .iter()
        .find_map(|f| match f {
            Fact::FunctionSignature {
                qualified_name,
                content_hash,
                ..
            } if qualified_name == "src.example.Greeter.greet" => Some(content_hash.clone()),
            _ => None,
        })
        .expect("greet fact must be present");

    // Mutating the body must NOT change content_hash.
    let mutated_body = SRC.replace(
        "return f\"{self.greeting}, {name}!\"",
        "return self.greeting + ', ' + name",
    );
    let ast_body = PythonAst::parse(mutated_body.as_bytes()).unwrap();
    let facts_body: Vec<Fact> =
        function_signature::extract(&ast_body, Path::new("src/example.py")).collect();
    let greet_body_hash = facts_body
        .iter()
        .find_map(|f| match f {
            Fact::FunctionSignature {
                qualified_name,
                content_hash,
                ..
            } if qualified_name == "src.example.Greeter.greet" => Some(content_hash.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        greet_hash, greet_body_hash,
        "body change leaked into signature hash"
    );

    // Mutating the signature MUST change content_hash.
    let mutated_sig = SRC.replace(
        "def greet(self, name: str) -> str:",
        "def greet(self, name: str) -> bytes:",
    );
    let ast_sig = PythonAst::parse(mutated_sig.as_bytes()).unwrap();
    let facts_sig: Vec<Fact> =
        function_signature::extract(&ast_sig, Path::new("src/example.py")).collect();
    let greet_sig_hash = facts_sig
        .iter()
        .find_map(|f| match f {
            Fact::FunctionSignature {
                qualified_name,
                content_hash,
                ..
            } if qualified_name == "src.example.Greeter.greet" => Some(content_hash.clone()),
            _ => None,
        })
        .unwrap();
    assert_ne!(
        greet_hash, greet_sig_hash,
        "signature change did not affect content_hash"
    );
}

#[test]
fn field_emits_one_per_class_attribute() {
    let ast = PythonAst::parse(SRC.as_bytes()).unwrap();
    let facts: Vec<Fact> = field::extract(&ast, Path::new("src/example.py")).collect();
    let fields: Vec<&Fact> = facts
        .iter()
        .filter(|f| matches!(f, Fact::Field { .. }))
        .collect();
    assert_eq!(
        fields.len(),
        1,
        "expected exactly one class field, got {fields:?}"
    );
    match fields[0] {
        Fact::Field {
            qualified_path,
            type_text,
            ..
        } => {
            assert_eq!(qualified_path, "src.example.Greeter.greeting");
            assert_eq!(type_text, "str");
        }
        _ => unreachable!(),
    }
}

#[test]
fn field_content_hash_covers_type_annotation() {
    let ast_before = PythonAst::parse(SRC.as_bytes()).unwrap();
    let facts_before: Vec<Fact> =
        field::extract(&ast_before, Path::new("src/example.py")).collect();
    let hash_before = facts_before
        .iter()
        .find_map(|f| match f {
            Fact::Field {
                qualified_path,
                content_hash,
                ..
            } if qualified_path == "src.example.Greeter.greeting" => Some(content_hash.clone()),
            _ => None,
        })
        .expect("greeting field must exist before");

    let mutated = SRC.replace("greeting: str = \"hello\"", "greeting: bytes = b\"hello\"");
    let ast_after = PythonAst::parse(mutated.as_bytes()).unwrap();
    let facts_after: Vec<Fact> = field::extract(&ast_after, Path::new("src/example.py")).collect();
    let hash_after = facts_after
        .iter()
        .find_map(|f| match f {
            Fact::Field {
                qualified_path,
                content_hash,
                ..
            } if qualified_path == "src.example.Greeter.greeting" => Some(content_hash.clone()),
            _ => None,
        })
        .expect("greeting field must exist after");

    assert_ne!(
        hash_before, hash_after,
        "type annotation change did not affect content_hash"
    );
}
