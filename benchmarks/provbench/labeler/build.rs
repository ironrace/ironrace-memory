use std::env;
use std::path::PathBuf;
use std::process::Command;

fn git_output(manifest_dir: &PathBuf, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(manifest_dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));
    if !out.status.success() {
        panic!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8(out.stdout)
        .unwrap_or_else(|e| panic!("git {args:?} returned non-UTF-8 stdout: {e}"))
        .trim()
        .to_string()
}

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());

    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("../../../.git/HEAD").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("../../../.git/index").display()
    );

    let sha = git_output(&manifest_dir, &["rev-parse", "HEAD"]);
    let dirty = !git_output(&manifest_dir, &["status", "--porcelain"]).is_empty();

    println!("cargo:rustc-env=PROVBENCH_LABELER_GIT_SHA={sha}");
    println!("cargo:rustc-env=PROVBENCH_LABELER_DIRTY={dirty}");
}
