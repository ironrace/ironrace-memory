#!/usr/bin/env bash
# check-versions.sh — verify that all plugin.json version fields match
# the canonical version in crates/ironrace-memory/Cargo.toml.
#
# Usage:
#   scripts/check-versions.sh          # exits 0 if all match, 1 on mismatch
#   scripts/check-versions.sh --fix    # rewrite plugin.json files to match Cargo.toml
#
# Run this in CI and before publishing a release.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

CARGO_TOML="${REPO_ROOT}/crates/ironrace-memory/Cargo.toml"
CODEX_PLUGIN="${REPO_ROOT}/.codex-plugin/plugin.json"
CLAUDE_PLUGIN="${REPO_ROOT}/.claude-plugin/plugin.json"

# ---------------------------------------------------------------------------
# Parse the canonical version from Cargo.toml
# ---------------------------------------------------------------------------
CARGO_VERSION=$(grep '^version = ' "${CARGO_TOML}" | head -1 | sed 's/version = "\(.*\)"/\1/')

if [[ -z "${CARGO_VERSION}" ]]; then
    echo "ERROR: could not parse version from ${CARGO_TOML}" >&2
    exit 1
fi

echo "Canonical version (Cargo.toml): ${CARGO_VERSION}"

# ---------------------------------------------------------------------------
# Check / fix each plugin.json
# ---------------------------------------------------------------------------
MISMATCH=0

check_or_fix() {
    local file="$1"
    local plugin_version
    plugin_version=$(grep '"version"' "${file}" | head -1 | sed 's/.*"version": *"\([^"]*\)".*/\1/')

    if [[ "${plugin_version}" == "${CARGO_VERSION}" ]]; then
        echo "  OK  ${file} (${plugin_version})"
    elif [[ "${FIX_MODE:-0}" == "1" ]]; then
        # In-place replacement using sed, works on both macOS and Linux
        if sed --version &>/dev/null 2>&1; then
            # GNU sed
            sed -i "s/\"version\": \"${plugin_version}\"/\"version\": \"${CARGO_VERSION}\"/" "${file}"
        else
            # BSD sed (macOS)
            sed -i '' "s/\"version\": \"${plugin_version}\"/\"version\": \"${CARGO_VERSION}\"/" "${file}"
        fi
        echo " FIXED ${file}: ${plugin_version} → ${CARGO_VERSION}"
    else
        echo "MISMATCH ${file}: found \"${plugin_version}\", expected \"${CARGO_VERSION}\"" >&2
        MISMATCH=1
    fi
}

FIX_MODE=0
if [[ "${1:-}" == "--fix" ]]; then
    FIX_MODE=1
fi

check_or_fix "${CODEX_PLUGIN}"
check_or_fix "${CLAUDE_PLUGIN}"

if [[ "${MISMATCH}" -ne 0 ]]; then
    echo ""
    echo "Run  scripts/check-versions.sh --fix  to update plugin.json files." >&2
    exit 1
fi

echo "All version fields are in sync."
