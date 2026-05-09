use provbench_labeler::tooling::{verify_binary_hash, ExpectedTool};
use std::io::Write;

#[test]
fn resolve_from_env_errors_when_neither_path_exists() {
    // Save state, override env to ensure neither binary is found,
    // then assert resolve_from_env returns an error mentioning "not found".
    let original_path = std::env::var_os("PATH");
    // Set PATH to a directory that exists but has no rust-analyzer/tree-sitter.
    let empty_dir = tempfile::tempdir().unwrap();
    // Safety: tests are single-threaded by default for this crate; if
    // parallelism becomes an issue we'll switch to an explicit serial guard.
    unsafe {
        std::env::set_var("PATH", empty_dir.path());
    }
    // Override the fallback path checks: the real /opt/homebrew/bin/rust-analyzer
    // may exist on this dev machine. Skip this test in that case.
    if std::path::Path::new("/opt/homebrew/bin/rust-analyzer").exists()
        || std::path::Path::new("/opt/homebrew/bin/tree-sitter").exists()
    {
        if let Some(p) = original_path {
            unsafe {
                std::env::set_var("PATH", p);
            }
        }
        eprintln!("skipping: real /opt/homebrew binary exists on this machine");
        return;
    }
    let result = provbench_labeler::tooling::resolve_from_env();
    if let Some(p) = original_path {
        unsafe {
            std::env::set_var("PATH", p);
        }
    }
    let err = result.expect_err("expected error when neither binary exists");
    assert!(err.to_string().contains("not found"), "got: {err}");
}

#[test]
fn rejects_binary_with_wrong_hash() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(b"not the real binary").unwrap();
    let path = tmp.path().to_path_buf();
    let expected = ExpectedTool {
        name: "fake",
        version_hint: "n/a",
        sha256_hex: "0000000000000000000000000000000000000000000000000000000000000000",
    };
    let err = verify_binary_hash(&path, &expected).unwrap_err();
    assert!(
        err.to_string().contains("hash mismatch"),
        "unexpected err: {err}"
    );
}

#[test]
fn accepts_binary_when_hash_matches() {
    use sha2::{Digest, Sha256};
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    let bytes = b"hello world";
    tmp.write_all(bytes).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hex = format!("{:x}", hasher.finalize());
    let expected = ExpectedTool {
        name: "fake",
        version_hint: "n/a",
        sha256_hex: Box::leak(hex.into_boxed_str()),
    };
    verify_binary_hash(tmp.path(), &expected).expect("hash should match");
}
