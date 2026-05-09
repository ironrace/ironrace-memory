use provbench_labeler::repo::{Pilot, PilotRepoSpec};
use std::path::PathBuf;

fn fixture_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_path_buf();
    let status = std::process::Command::new("git")
        .args(["init", "--initial-branch=main", path.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    let write_and_commit = |name: &str, body: &str, msg: &str| {
        std::fs::write(path.join(name), body).unwrap();
        let s1 = std::process::Command::new("git")
            .args(["-C", path.to_str().unwrap(), "add", name])
            .status()
            .unwrap();
        assert!(s1.success());
        let s2 = std::process::Command::new("git")
            .args([
                "-C",
                path.to_str().unwrap(),
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-m",
                msg,
            ])
            .status()
            .unwrap();
        assert!(s2.success());
    };
    write_and_commit("a.rs", "fn one() {}\n", "c1");
    write_and_commit("a.rs", "fn one_renamed() {}\n", "c2");
    write_and_commit("a.rs", "fn one_renamed() { let x = 1; }\n", "c3");
    (tmp, path)
}

#[test]
fn walks_first_parent_from_t0() {
    let (_keep, path) = fixture_repo();
    let head = std::process::Command::new("git")
        .args([
            "-C",
            path.to_str().unwrap(),
            "rev-list",
            "--max-parents=0",
            "HEAD",
        ])
        .output()
        .unwrap();
    let t0 = String::from_utf8(head.stdout).unwrap().trim().to_string();
    let pilot = Pilot::open(&PilotSpecLocal {
        path: path.clone(),
        t0_sha: t0.clone(),
    })
    .unwrap();
    let commits: Vec<_> = pilot.walk_first_parent().unwrap().collect();
    assert_eq!(commits.len(), 3, "got {commits:?}");
    assert_eq!(commits[0].sha, t0);
}

#[test]
fn walk_errors_when_t0_not_in_first_parent_chain() {
    let (_keep, path) = fixture_repo();
    // Use a SHA that parses as valid hex but is not in the repo.
    let bogus_t0 = "0000000000000000000000000000000000000001";
    let spec = PilotSpecLocal {
        path,
        t0_sha: bogus_t0.to_string(),
    };
    // Pilot::open already errors when T₀ is missing from the ODB; that's
    // the right place to catch this. Verify the message names the SHA.
    match Pilot::open(&spec) {
        Ok(_) => panic!("expected Pilot::open to fail for bogus T₀"),
        Err(e) => {
            let msg = e.to_string();
            assert!(msg.contains(bogus_t0), "got: {msg}");
        }
    }
}

struct PilotSpecLocal {
    path: PathBuf,
    t0_sha: String,
}

impl PilotRepoSpec for PilotSpecLocal {
    fn local_clone_path(&self) -> &std::path::Path {
        &self.path
    }
    fn t0_sha(&self) -> &str {
        &self.t0_sha
    }
}
