use provbench_labeler::ast::python::PythonAst;
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
