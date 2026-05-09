use provbench_labeler::tooling::{verify_binary_hash, ExpectedTool};
use std::io::Write;

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
