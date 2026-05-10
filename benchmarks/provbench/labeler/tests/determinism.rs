use provbench_labeler::output::{write_jsonl, OutputRow};
use provbench_labeler::replay::{Replay, ReplayConfig};

#[test]
fn two_runs_produce_byte_identical_output() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    let g = |args: &[&str]| {
        let s = std::process::Command::new("git")
            .args(args)
            .current_dir(p)
            .status()
            .unwrap();
        assert!(s.success(), "git {args:?}");
    };
    g(&["init", "--initial-branch=main"]);
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        b"[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(p.join("src/lib.rs"), b"pub fn ten() -> i32 { 10 }\n").unwrap();
    g(&["add", "."]);
    g(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-m",
        "init",
    ]);
    let t0 = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(p)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    std::fs::write(p.join("src/lib.rs"), b"pub fn ten() -> i32 { 11 }\n").unwrap();
    g(&["add", "."]);
    g(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-m",
        "tweak",
    ]);

    let cfg = ReplayConfig {
        repo_path: p.to_path_buf(),
        t0_sha: t0.clone(),
        skip_symbol_resolution: true,
    };
    let rows1 = Replay::run(&cfg).unwrap();
    let rows2 = Replay::run(&cfg).unwrap();
    let out1 = tempfile::NamedTempFile::new().unwrap();
    let out2 = tempfile::NamedTempFile::new().unwrap();
    let to_output = |rs: Vec<provbench_labeler::replay::FactAtCommit>| -> Vec<OutputRow> {
        rs.into_iter()
            .map(|r| OutputRow {
                fact_id: r.fact_id,
                commit_sha: r.commit_sha,
                label: r.label,
            })
            .collect()
    };
    write_jsonl(out1.path(), &to_output(rows1), "stamp").unwrap();
    write_jsonl(out2.path(), &to_output(rows2), "stamp").unwrap();
    let b1 = std::fs::read(out1.path()).unwrap();
    let b2 = std::fs::read(out2.path()).unwrap();
    assert_eq!(b1, b2, "labeler is non-deterministic");
}
