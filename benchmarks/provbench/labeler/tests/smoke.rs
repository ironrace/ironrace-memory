#[test]
fn binary_prints_version_when_invoked_with_dash_v() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_provbench-labeler"))
        .arg("--version")
        .output()
        .expect("run labeler");
    assert!(
        out.status.success(),
        "labeler --version exited non-zero: {:?}",
        out
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("provbench-labeler"),
        "missing name: {stdout}"
    );
}
