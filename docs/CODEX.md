# Codex Guide

## Purpose

This guide explains how to use `ironrace-memory` with Codex today, what is still missing, and how to compare it against `mempalace`.

## Current Support Level

What works now:

- Running `ironmem` as an MCP server over stdio
- Read and write MCP tools
- Semantic search
- Knowledge graph tools
- Restricted vs trusted access modes
- `mine` for workspace ingestion with incremental updates
- `hook` for session-start, stop, and precompact
- Codex plugin packaging
- Automatic migrate-or-init bootstrap on first use

What does not work yet:

- Release/distribution polish is still thin
- Hook behavior does not yet build a rich LLM-written session summary from transcript content
- Install behavior is plugin-wrapper based, not a separate installer command

## Build

From the repo root:

```bash
cargo build -p ironrace-memory --bin ironmem
./target/debug/ironmem setup
```

`setup` prepares the embedding model under the default model cache. On a fresh machine it may download the model.

## Manual Codex MCP Setup

Add a server entry to your Codex MCP config.

Example `~/.codex/config.toml` fragment:

```toml
[mcp_servers.ironrace_memory]
command = "/Users/jeffreycrum/git-repos/ironrace-memory/target/debug/ironmem"
args = ["serve"]

[mcp_servers.ironrace_memory.env]
IRONMEM_MCP_MODE = "trusted"
IRONMEM_DB_PATH = "/Users/jeffreycrum/.ironrace-memory/memory.sqlite3"
```

If you want a project-local store instead of the default home-directory location, point `IRONMEM_DB_PATH` at a repo-local path.

## Manual Validation

After registering the MCP server, validate the basics:

1. Start Codex and confirm the server appears in MCP listings.
2. Call `ironmem_status`.
3. Add a small drawer with `ironmem_add_drawer`.
4. Search for it with `ironmem_search`.

## Operational Notes

- `IRONMEM_MCP_MODE=trusted` enables writes.
- `IRONMEM_MCP_MODE=read-only` disables write tools.
- `IRONMEM_MCP_MODE=restricted` disables writes and redacts sensitive returned content.
- Mining skips hidden files and directories by default. Set `IRONMEM_MINE_HIDDEN=1` only when you explicitly want dot-paths indexed.

## Codex Packaging Gap

`ironrace-memory` now ships a `.codex-plugin/` directory with:

- `plugin.json`
- `hooks.json`
- wrapper scripts for the MCP server and hooks
- Codex-specific README content

The hook wrapper delegates to:

```bash
ironmem hook session-start --harness codex
ironmem hook stop --harness codex
ironmem hook precompact --harness codex
```

## Install-Time Migration and Bootstrap

Current behavior:

- If the user already has `mempalace`, installation detects that state and migrates automatically
- If the user does not have `mempalace`, installation initializes a fresh store automatically
- The embedding model is prepared automatically
- The active workspace is mined automatically on first use when the plugin wrapper can infer a workspace root
- Later hook runs update memory incrementally rather than re-mining everything

## Continuous Updates

Current behavior for Codex:

- first run: bootstrap, migrate-or-init, initial mine
- `PreCompact`: save summary and ingest changed files
- `Stop`: save durable summary and ingest changed files
- later sessions: query memory first when historical context matters

## Memory Usage Guidance

Codex should get a short protocol reminding it to use memory proactively.

Recommended text:

> Before answering questions about prior work, decisions, project history, or people, check `ironmem_search` or the KG tools first. After important progress or decisions, write durable summaries back into memory.

Best places to inject this:

- `.codex-plugin/plugin.json` default prompt metadata
- `ironmem_status` response
- Codex-facing README/setup docs

## Benchmarking Against MemPalace

This repo now includes a benchmark harness:

```bash
python3 scripts/benchmark_vs_mempalace.py --help
```

Example run:

```bash
python3 scripts/benchmark_vs_mempalace.py \
  --documents 100 \
  --queries 15 \
  --runs 2 \
  --output-json /tmp/ironmem-vs-mempalace.json
```

The script:

- launches both MCP servers
- ingests the same synthetic dataset through each server's `add_drawer` tool
- measures startup, ingest, search, taxonomy, status, and delete latency
- records simple hit-rate checks for planted search needles

Defaults:

- `ironrace-memory` repo: current working tree
- `mempalace` repo: `~/git-repos/mempalace`

## Benchmark Caveats

- `ironrace-memory` uses a Rust ONNX embedding path
- `mempalace` uses its own Python and Chroma stack
- First-run model/bootstrap costs may dominate small workloads
- File mining is implemented in `ironrace-memory`, but this harness still avoids file-level comparisons because the mining pipelines differ and the tool-driven benchmark is more controlled

## Recommended Next Work

1. Make env-sensitive runtime tests safe under parallel execution
2. Add MCP smoke tests in CI
3. Extend benchmark coverage with larger datasets and repeated warm-cache runs
4. Tighten mining defaults for sensitive local config surfaces
