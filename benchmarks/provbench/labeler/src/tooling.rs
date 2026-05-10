//! Tooling-pin verification per SPEC §13.1.
//!
//! Phase 0b labels are invalid unless every external tool used at label
//! time matches the binary content hash recorded in the spec freeze. A
//! version-string match alone is **not** sufficient — distros patch.
//!
//! ## Supported platforms
//!
//! - `aarch64-darwin` (macOS, Apple Silicon): canonical dev / freeze
//!   environment. Hashes match the SPEC §13.1 record (rustup-installed
//!   rust-analyzer; Homebrew tree-sitter).
//! - `x86_64-unknown-linux-gnu` (`ubuntu-latest` GitHub runner): hashes
//!   correspond to the **decompressed** binaries published as
//!   `*.gz` upstream release artifacts. CI must install the tools by
//!   downloading those `.gz` artifacts and gunzipping (rather than
//!   apt/rustup), so on-disk bytes match the upstream-published binary.
//!
//! `x86_64-darwin` and `aarch64-linux` are explicitly out of scope for
//! this hardening pass. Adding them requires verified hashes from the
//! same upstream artifacts.

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct ExpectedTool {
    pub name: &'static str,
    pub version_hint: &'static str,
    pub sha256_hex: &'static str,
}

/// Pinned-binary entry for a single (os, arch, tool) tuple.
///
/// `fallback_path` is consulted only when the tool is not on `PATH`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PinnedBinary {
    pub target_os: &'static str,
    pub target_arch: &'static str,
    pub binary_name: &'static str,
    pub tool: ExpectedTool,
    pub fallback_path: &'static str,
}

/// Pinned-hash table indexed by (target_os, target_arch, binary_name).
///
/// Each row records the upstream artifact whose decompressed bytes
/// produce `tool.sha256_hex`. Add new rows only after running
/// `shasum -a 256` on the actual artifact — never copy hashes from
/// secondary sources.
pub(crate) const PINNED_BINARIES: &[PinnedBinary] = &[
    // ---- aarch64-darwin: SPEC §13.1 freeze record. -----------------
    // rust-analyzer 1.85.0 (4d91de4e 2025-02-17), rustup
    // stable-aarch64-apple-darwin component.
    PinnedBinary {
        target_os: "macos",
        target_arch: "aarch64",
        binary_name: "rust-analyzer",
        tool: ExpectedTool {
            name: "rust-analyzer",
            version_hint: "1.85.0 (4d91de4e 2025-02-17)",
            sha256_hex: "f85740bfa5b9136e9053768c015c31a6c7556f7cfe44f7f9323965034e1f9aee",
        },
        fallback_path: "/opt/homebrew/bin/rust-analyzer",
    },
    // tree-sitter 0.25.6, Homebrew binary at /opt/homebrew/bin/tree-sitter.
    PinnedBinary {
        target_os: "macos",
        target_arch: "aarch64",
        binary_name: "tree-sitter",
        tool: ExpectedTool {
            name: "tree-sitter",
            version_hint: "0.25.6",
            sha256_hex: "3e82f0982232f68fd5b0192caf4bb06064cc034f837552272eec8d67014edc5c",
        },
        fallback_path: "/opt/homebrew/bin/tree-sitter",
    },
    // ---- x86_64-linux-gnu: ubuntu-latest CI. -----------------------
    // rust-analyzer 1.85.0 — decompressed
    // `rust-analyzer-x86_64-unknown-linux-gnu.gz` from the
    // `2025-02-17` GitHub release. URL:
    //   https://github.com/rust-lang/rust-analyzer/releases/download/2025-02-17/rust-analyzer-x86_64-unknown-linux-gnu.gz
    // Verified locally: `gunzip` then `shasum -a 256` →
    //   e7a85d27756b595be0054af90bd5f1e0420ef2e8c60782e42146bbe4765f7410
    PinnedBinary {
        target_os: "linux",
        target_arch: "x86_64",
        binary_name: "rust-analyzer",
        tool: ExpectedTool {
            name: "rust-analyzer",
            version_hint: "1.85.0 (4d91de4e 2025-02-17)",
            sha256_hex: "e7a85d27756b595be0054af90bd5f1e0420ef2e8c60782e42146bbe4765f7410",
        },
        fallback_path: "/usr/local/bin/rust-analyzer",
    },
    // tree-sitter 0.25.6 — decompressed `tree-sitter-linux-x64.gz`
    // from the v0.25.6 GitHub release. URL:
    //   https://github.com/tree-sitter/tree-sitter/releases/download/v0.25.6/tree-sitter-linux-x64.gz
    // Verified locally: `gunzip` then `shasum -a 256` →
    //   274404803072a504b7e31a0d8fde02d50146b688155a12429f73ed35be30d95e
    PinnedBinary {
        target_os: "linux",
        target_arch: "x86_64",
        binary_name: "tree-sitter",
        tool: ExpectedTool {
            name: "tree-sitter",
            version_hint: "0.25.6",
            sha256_hex: "274404803072a504b7e31a0d8fde02d50146b688155a12429f73ed35be30d95e",
        },
        fallback_path: "/usr/local/bin/tree-sitter",
    },
];

/// Human-readable list of supported `(os, arch)` platforms, used in
/// the unsupported-platform error message.
pub(crate) const SUPPORTED_PLATFORMS: &[&str] = &["aarch64-darwin", "x86_64-linux-gnu"];

pub fn verify_binary_hash(path: &Path, expected: &ExpectedTool) -> Result<()> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read {} at {}", expected.name, path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected.sha256_hex {
        return Err(anyhow!(
            "tooling hash mismatch for {}: expected {} (version {}), got {}",
            expected.name,
            expected.sha256_hex,
            expected.version_hint,
            actual
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ResolvedTooling {
    pub rust_analyzer: std::path::PathBuf,
    pub tree_sitter: std::path::PathBuf,
}

fn resolve_binary(name: &str, fallback: &str) -> Result<std::path::PathBuf> {
    let path = match which::which(name) {
        Ok(p) => p,
        Err(_) => std::path::PathBuf::from(fallback),
    };
    if !path.exists() {
        anyhow::bail!("{name} not found on PATH and not present at {fallback}");
    }
    Ok(path)
}

/// Look up the pinned entry for `binary_name` on the given platform.
///
/// Returns `None` when the platform is unsupported or the binary is
/// not pinned for that platform. Callers translate `None` into a hard
/// error that lists `SUPPORTED_PLATFORMS`.
pub(crate) fn pinned_for(
    target_os: &str,
    target_arch: &str,
    binary_name: &str,
) -> Option<&'static PinnedBinary> {
    PINNED_BINARIES.iter().find(|row| {
        row.target_os == target_os
            && row.target_arch == target_arch
            && row.binary_name == binary_name
    })
}

/// Build the unsupported-platform error message. Extracted so tests
/// can exercise it without depending on the real host platform.
pub(crate) fn unsupported_platform_error(target_os: &str, target_arch: &str) -> anyhow::Error {
    anyhow!(
        "unsupported platform {}-{} for provbench labeler tooling pins; \
         supported platforms: {}",
        target_arch,
        target_os,
        SUPPORTED_PLATFORMS.join(", "),
    )
}

/// Resolve and hash-verify a single pinned binary for the given
/// platform. Hard-fails if the platform is unsupported.
pub(crate) fn resolve_one(
    target_os: &str,
    target_arch: &str,
    binary_name: &str,
) -> Result<std::path::PathBuf> {
    let pinned = pinned_for(target_os, target_arch, binary_name)
        .ok_or_else(|| unsupported_platform_error(target_os, target_arch))?;
    let path = resolve_binary(pinned.binary_name, pinned.fallback_path)?;
    verify_binary_hash(&path, &pinned.tool)?;
    Ok(path)
}

pub fn resolve_from_env() -> Result<ResolvedTooling> {
    let target_os = std::env::consts::OS;
    let target_arch = std::env::consts::ARCH;
    // Fail loudly *before* any binary lookup so the error names the
    // platform rather than a missing binary.
    if pinned_for(target_os, target_arch, "rust-analyzer").is_none()
        || pinned_for(target_os, target_arch, "tree-sitter").is_none()
    {
        return Err(unsupported_platform_error(target_os, target_arch));
    }
    let rust_analyzer = resolve_one(target_os, target_arch, "rust-analyzer")?;
    let tree_sitter = resolve_one(target_os, target_arch, "tree-sitter")?;
    Ok(ResolvedTooling {
        rust_analyzer,
        tree_sitter,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_table_covers_supported_platforms() {
        // Every (platform, binary) cell must be present so
        // `resolve_from_env()` never half-succeeds on a supported host.
        for platform in [("macos", "aarch64"), ("linux", "x86_64")] {
            for bin in ["rust-analyzer", "tree-sitter"] {
                assert!(
                    pinned_for(platform.0, platform.1, bin).is_some(),
                    "missing pinned entry for {}-{} {}",
                    platform.1,
                    platform.0,
                    bin,
                );
            }
        }
    }

    #[test]
    fn pinned_table_has_no_unsupported_targets() {
        // Guard against accidentally adding a row whose (os, arch)
        // pair is not also listed in SUPPORTED_PLATFORMS.
        for row in PINNED_BINARIES {
            let label = match (row.target_os, row.target_arch) {
                ("macos", "aarch64") => "aarch64-darwin",
                ("linux", "x86_64") => "x86_64-linux-gnu",
                other => panic!("unexpected pinned target {other:?}"),
            };
            assert!(
                SUPPORTED_PLATFORMS.contains(&label),
                "{label} missing from SUPPORTED_PLATFORMS"
            );
        }
    }

    #[test]
    fn pinned_hashes_are_64_hex_chars() {
        for row in PINNED_BINARIES {
            let h = row.tool.sha256_hex;
            assert_eq!(h.len(), 64, "{} hash wrong length: {h}", row.binary_name);
            assert!(
                h.chars().all(|c| c.is_ascii_hexdigit()),
                "{} hash contains non-hex: {h}",
                row.binary_name
            );
        }
    }

    #[test]
    fn unsupported_platform_error_lists_both_targets() {
        // Reachable on any host because we pass the platform args
        // explicitly rather than reading std::env::consts::OS.
        let err = unsupported_platform_error("freebsd", "riscv64");
        let msg = err.to_string();
        assert!(msg.contains("freebsd"), "missing host os: {msg}");
        assert!(msg.contains("riscv64"), "missing host arch: {msg}");
        assert!(msg.contains("aarch64-darwin"), "missing macos pin: {msg}");
        assert!(msg.contains("x86_64-linux-gnu"), "missing linux pin: {msg}");
    }

    #[test]
    fn resolve_one_rejects_unsupported_platform() {
        // `resolve_one` short-circuits on unknown platforms before
        // touching the filesystem, so the test is host-agnostic.
        let err = resolve_one("freebsd", "riscv64", "rust-analyzer").unwrap_err();
        assert!(
            err.to_string().contains("unsupported platform"),
            "expected unsupported-platform error, got: {err}"
        );
    }

    #[test]
    fn resolve_one_rejects_unknown_binary() {
        let err = resolve_one("macos", "aarch64", "definitely-not-a-tool").unwrap_err();
        assert!(
            err.to_string().contains("unsupported platform"),
            "expected unsupported-platform error for unknown binary, got: {err}"
        );
    }
}
