//! Build script that embeds a git-describe-augmented version string into the
//! binary as the `IRONMEM_VERSION` environment variable.
//!
//! Output format:
//!   - At a clean release tag (`vX.Y.Z` matching `CARGO_PKG_VERSION`): just
//!     the cargo version, e.g. `0.2.0`.
//!   - Anywhere else: cargo version plus the git-describe suffix, e.g.
//!     `0.1.0 (26eda5bcd124)` or `0.1.0 (26eda5bcd124-dirty)`.
//!
//! Falls back to `CARGO_PKG_VERSION` alone when `git` is unavailable or the
//! source tree has no `.git` directory (e.g. crates.io tarball install).

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let pkg_version =
        std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is set by cargo");

    let version = match git_describe() {
        Some(info) if info == format!("v{pkg_version}") || info == pkg_version => pkg_version,
        Some(info) => format!("{pkg_version} ({info})"),
        None => pkg_version,
    };
    println!("cargo:rustc-env=IRONMEM_VERSION={version}");

    // Rebuild when git state changes so the embedded version stays current.
    // Walk up from the crate manifest dir to find the workspace `.git`.
    if let Some(git_dir) = find_git_dir() {
        let head = git_dir.join("HEAD");
        if head.exists() {
            println!("cargo:rerun-if-changed={}", head.display());
        }
        let packed = git_dir.join("packed-refs");
        if packed.exists() {
            println!("cargo:rerun-if-changed={}", packed.display());
        }
        for sub in ["refs/heads", "refs/tags"] {
            let p = git_dir.join(sub);
            if p.exists() {
                println!("cargo:rerun-if-changed={}", p.display());
            }
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}

fn git_describe() -> Option<String> {
    let out = Command::new("git")
        .args([
            "describe",
            "--tags",
            "--always",
            "--dirty",
            "--abbrev=12",
            "--first-parent",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn find_git_dir() -> Option<PathBuf> {
    let mut p = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?);
    loop {
        let candidate = p.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        // Also handle worktrees / submodules where `.git` is a file pointing
        // at the real gitdir. `git describe` itself handles both cases; we
        // just need *something* to watch for rerun-if-changed.
        if candidate.is_file() {
            return None;
        }
        if !p.pop() {
            return None;
        }
    }
}
