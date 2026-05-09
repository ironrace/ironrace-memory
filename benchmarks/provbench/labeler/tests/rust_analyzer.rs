use provbench_labeler::resolve::rust_analyzer::RustAnalyzer;
use provbench_labeler::resolve::SymbolResolver;

#[test]
#[ignore = "requires rust-analyzer on PATH; run with `cargo test -- --ignored`"]
fn resolves_pub_fn_in_minimal_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        b"[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), b"pub fn marker_fn() {}\n").unwrap();
    let mut ra = RustAnalyzer::spawn(tmp.path()).unwrap();
    let resolved = ra.resolve("marker_fn").unwrap();
    assert!(resolved.is_some(), "marker_fn should resolve");
}
