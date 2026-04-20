#!/usr/bin/env bash
# install-ironmem.sh — atomically install the ironmem binary to ~/.ironrace/bin/
#
# Why this script exists: plain `cp` overwrites bytes in place, same inode.
# macOS lets that happen even while an `ironmem serve` process is actively
# executing the file; the write corrupts the running code page and any new
# invocation loading the same inode silently hangs or exits. Using install(1)
# unlinks the old file and creates a new one, so running processes keep their
# old copy and new invocations get a clean binary.
#
# The script builds release (unless --skip-build), then atomically replaces
# ~/.ironrace/bin/ironmem and verifies the resulting binary runs.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

INSTALL_DIR="${IRONMEM_INSTALL_DIR:-$HOME/.ironrace/bin}"
TARGET="$INSTALL_DIR/ironmem"
SOURCE="$REPO_ROOT/target/release/ironmem"

SKIP_BUILD=0
if [[ "${1:-}" == "--skip-build" ]]; then
  SKIP_BUILD=1
fi

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  echo "==> Building ironmem release"
  (cd "$REPO_ROOT" && cargo build --release -p ironmem --bin ironmem)
fi

if [[ ! -x "$SOURCE" ]]; then
  echo "ERROR: release binary not found at $SOURCE" >&2
  echo "Run without --skip-build, or build manually first." >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"

echo "==> Installing $SOURCE → $TARGET (atomic)"
# install(1) unlinks the target and creates a fresh inode, safe for running
# processes. `-m 755` sets executable bits; `-C` is a no-op copy if identical.
install -m 755 "$SOURCE" "$TARGET"

echo "==> Verifying installed binary"
if ! VERSION_OUTPUT=$("$TARGET" --version 2>&1); then
  echo "ERROR: installed binary at $TARGET failed to run" >&2
  echo "$VERSION_OUTPUT" >&2
  exit 1
fi
echo "    $VERSION_OUTPUT"

# Surface running `ironmem serve` instances as an FYI — the atomic install
# does not disturb them, but callers that want new clients to hit the fresh
# binary must restart their MCP client (Claude Code, Codex, etc).
RUNNING="$(pgrep -f 'ironmem serve' 2>/dev/null || true)"
if [[ -n "$RUNNING" ]]; then
  echo ""
  echo "Note: running ironmem serve process(es) detected (PIDs: $RUNNING)."
  echo "      They continue on the previous binary. Restart your MCP client"
  echo "      (Claude Code / Codex) to reconnect to the freshly installed one."
fi

# Detect legacy MCP server registrations left over from the pre-rename era
# (ironrace-memory → ironmem). We do NOT edit these files — a legacy entry
# may point at a forked or staging binary deliberately. Warn only, with the
# exact command to remove it.
CLAUDE_CONFIG="$HOME/.claude.json"
CODEX_CONFIG="$HOME/.codex/config.toml"
LEGACY_FOUND=0

if [[ -f "$CLAUDE_CONFIG" ]] && command -v jq >/dev/null 2>&1; then
  if jq -e '.mcpServers["ironrace-memory"]' "$CLAUDE_CONFIG" >/dev/null 2>&1; then
    if [[ "$LEGACY_FOUND" -eq 0 ]]; then echo ""; echo "Legacy MCP registrations detected:"; fi
    LEGACY_FOUND=1
    echo "  - Claude Code ($CLAUDE_CONFIG) has an 'ironrace-memory' server."
    echo "      Remove with: claude mcp remove ironrace-memory"
  fi
fi

if [[ -f "$CODEX_CONFIG" ]] && grep -q '^\[mcp_servers\.ironrace_memory\]' "$CODEX_CONFIG" 2>/dev/null; then
  if [[ "$LEGACY_FOUND" -eq 0 ]]; then echo ""; echo "Legacy MCP registrations detected:"; fi
  LEGACY_FOUND=1
  echo "  - Codex ($CODEX_CONFIG) has an [mcp_servers.ironrace_memory] section."
  echo "      Remove it by hand — delete the [mcp_servers.ironrace_memory] block"
  echo "      and any [mcp_servers.ironrace_memory.*] subsections."
fi

if [[ "$LEGACY_FOUND" -eq 1 ]]; then
  echo ""
  echo "  Why this matters: the plugin registers itself as 'ironmem'. When a"
  echo "  legacy 'ironrace-memory' server is also registered, tool calls render"
  echo "  under the old name and both servers run against the same SQLite DB."
fi

echo "==> Done"
