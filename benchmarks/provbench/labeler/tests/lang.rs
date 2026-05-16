use provbench_labeler::lang::Language;
use std::path::Path;

#[test]
fn for_path_rust() {
    assert_eq!(Language::for_path(Path::new("src/lib.rs")), Some(Language::Rust));
}

#[test]
fn for_path_python() {
    assert_eq!(Language::for_path(Path::new("src/app.py")), Some(Language::Python));
}

#[test]
fn for_path_markdown_is_none() {
    // Markdown is handled separately by doc-claim extractors, not Language.
    assert_eq!(Language::for_path(Path::new("README.md")), None);
}

#[test]
fn for_path_unknown_extension() {
    assert_eq!(Language::for_path(Path::new("data.toml")), None);
    assert_eq!(Language::for_path(Path::new("Makefile")), None);
}

#[test]
fn source_extensions_is_stable_sorted() {
    let exts = Language::source_extensions();
    let mut owned: Vec<&str> = exts.to_vec();
    owned.sort();
    assert_eq!(owned, exts);
}

#[test]
fn extension_roundtrips_with_for_path() {
    use std::path::PathBuf;
    for lang in [Language::Rust, Language::Python] {
        let path = PathBuf::from(format!("x.{}", lang.extension()));
        assert_eq!(Language::for_path(&path), Some(lang));
    }
}

#[test]
fn source_extensions_covers_all_variants() {
    use std::collections::HashSet;
    let exts: HashSet<&str> = Language::source_extensions().iter().copied().collect();
    for lang in [Language::Rust, Language::Python] {
        assert!(
            exts.contains(lang.extension()),
            "source_extensions() missing entry for {:?}",
            lang
        );
    }
}
