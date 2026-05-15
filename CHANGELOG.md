# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **provbench-baseline** (Phase 0c): new workspace-excluded crate implementing
  the LLM-as-invalidator baseline against `claude-sonnet-4-6` snapshot
  2026-05-09 per SPEC §6.1. Three subcommands (`sample`, `run`, `score`).
  Operational $25 budget cap (preflight + live abort) under the spec's
  immutable $250 ceiling. Stratified sampler, atomic checkpointing,
  `--resume`, schema-derived preflight estimator, prompt caching at the
  static prefix, parse-error addendum retry, §7.1 three-way metrics +
  §9.2 LLM-validator agreement with Wilson intervals + Cohen κ bootstrap.
- **provbench-labeler**: two new subcommands `emit-facts` and `emit-diffs`
  to produce the JSON artifacts consumed by the baseline runner.
- **provbench-phase1** (Phase 1): new workspace-excluded crate implementing
  the rules-based structural invalidator (`rule_set_version v1.0` →
  `v1.1`, frozen at phase1 git SHA `ccfc901be171`). 7-rule chain
  (`source_file_missing`, `blob_identical`, `symbol_missing`,
  `span_hash_changed`, `whitespace_or_comment_only`, `doc_claim`,
  `rename_candidate`), deterministic single-repo HEAD-only replay,
  per-rule confusion + audit trail in `rule_traces.jsonl`. Pilot v1.1
  clears SPEC §8 #3 / #4 / #5 on the ripgrep Phase 0c canary
  (n=4,387; WLB valid 0.9716, p50 2 ms, WLB stale 0.9537).
- **provbench-scoring**: shared SPEC §7 math crate (Wilson intervals,
  three-way confusion, F1, Cohen κ bootstrap) split out of baseline
  so phase1 and baseline both consume the same scorer. `compare`
  subcommand produces side-by-side `metrics.json` with deltas.
- **ProvBench §9.4 held-out evaluation — Round 1 (serde-rs/serde @
  T₀ `65e1a507`, v1.0.130).** First held-out evaluation of phase1
  v1.1 against a repo the rules were never tuned on (SPEC §13.2
  pre-registered, leakage-clean). Result: **FAIL §8 #3** — valid
  retention WLB 0.9062 < 0.95 required; pilot was 0.9716 (−6.5pp
  drop). §8 #4 latency p50 = 14 ms (PASS) and §8 #5 stale recall
  WLB = 0.9391 (PASS) generalize cleanly. Per-rule confusion
  attributes the §8 #3 miss to R4 (`span_hash_changed` line-presence
  probe): held-out false-Stale on GT=Valid is 162 vs pilot 17 (10×
  pilot rate). Per SPEC §10 no in-round retuning; SPEC §11 row
  records the FAIL. A future v1.2 with retuned R4 would re-run the
  leakage clock against pallets/flask (Round 2; pre-registered).
  Findings:
  `benchmarks/provbench/results/serde-heldout-2026-05-15-findings.md`.

### Fixed

- **ProvBench labeler — Phase 0b hardening pass 5 (2026-05-13).**
  Three structural fixes addressing the post-pass-4 spot-check
  findings (`benchmarks/provbench/spotcheck/2026-05-13-post-pass4-findings.md`):
  (1) `FunctionSignature` post-commit pairing now uses a private
  replay-time disambiguator keyed on `(qualified_name,
  cfg_attribute_set, impl_receiver_type)` with a zero-based ordinal
  tiebreaker, mirroring pass-4's `TestAssertion` ordinal fix. When a
  T₀ fact's specific cfg/impl variant is deleted at a later commit
  while same-qualified-name survivors exist in other variants, the
  row routes to `NeedsRevalidation` (gray area for LLM follow-up)
  instead of mis-pairing against a survivor's span/hash and emitting
  `StaleSourceChanged`. ~9 sample rows fix.
  (2) `PublicSymbol` bare `pub use` re-exports (including
  `pub use … Original as Alias`) now preserve public-surface
  continuity → `Valid`, even when the post declaration span hashes
  differently from a T₀ definition span. Restricted-visibility uses
  (`pub(crate) use`, `pub(super) use`, `pub(in …) use`) remain
  narrowed → `StaleSourceChanged` via the pass-3 visibility-narrowing
  path. Glob re-exports (`pub use path::*;`) remain out of scope.
  ~2 sample rows fix.
  (3) `Fact::Field` post-commit matching now consults a private
  file-local `same_file_leaf_elsewhere` helper. When the T₀ field's
  exact `qualified_path` no longer resolves but the same leaf name
  appears in another struct or enum-variant in the same file, the
  row routes to `NeedsRevalidation` (file-local restructure gray
  area). Cross-file field-leaf tracking is intentionally not
  extended into `CommitSymbolIndex`. ~3 sample rows fix.
  The `Fact` enum, JSONL schema, and `fact_id` format are all
  byte-stable across this pass. `sample-eaf82d2.csv` remains the
  diagnostic ground-truth for the pass-4 gate FAIL; SPEC §9.1
  acceptance requires a freshly regenerated corpus + new-seed sample
  post-merge.

- **ProvBench labeler — Phase 0b hardening pass 4 (2026-05-13).**
  Two structural fixes addressing the post-pass-3 spot-check findings
  (`benchmarks/provbench/spotcheck/2026-05-12-post-pass3-findings.md`):
  (1) `TestAssertion` post-commit pairing was matching by `test_fn`
  alone via `find_map`, returning the first assertion in the
  post-commit test fn for every T₀ fact in that fn. Non-first
  assertions in a multi-assertion `#[test]` body silently routed to
  `StaleSourceChanged` even in byte-identical files. Pairing now uses
  `(test_fn, zero-based ordinal)` via a private replay-time
  disambiguator on `ObservedFact`; the `Fact` enum, JSONL schema, and
  `fact_id` format are byte-stable. Blast radius across the ripgrep
  pilot corpus before this fix: 80.7% of `TestAssertion` fact_ids
  (667/827) were subject to misclassification.
  (2) Added a SPEC §5 byte-identical-file structural guardrail in
  `Replay::run_inner` step 3: when a fact's source path is
  byte-identical between T₀ and `commit_sha`, every fact at that path
  classifies `Valid` without invoking per-fact matching, symbol
  resolution, rename detection, or whitespace/comment diffing.
  Defense-in-depth: catches per-fact-matcher ambiguity for all five
  fact kinds (including `DocClaim` on byte-identical markdown), and
  structurally covers the lone `FunctionSignature::is_hidden` outlier
  from the pre-merge sample without chasing its per-fact root cause.
  `sample-e96c9fe.csv` was drawn against the buggy corpus and is
  diagnostic-only; the SPEC §9.1 acceptance gate must be re-run on a
  freshly regenerated corpus drawn with a NEW seed.

### Added

- **ProvBench labeler — `spotcheck --seed <u64>` (2026-05-12).** The
  stratified sampler now accepts an optional seed (decimal or `0x`-
  prefixed hex) so post-merge / anti-tuning validation runs can draw a
  fresh sample against a regenerated corpus. Omitting `--seed` uses
  the new `DEFAULT_SEED` public constant
  (`0xC0DEBABEDEADBEEF`, the historical value), preserving
  byte-identical replay for resuming an in-progress reviewer CSV. The
  CLI echoes the resolved seed and writes a `<out>.meta.json` sidecar
  recording `{corpus, seed, n, labeler_git_sha}` so the on-disk
  spot-check artifact is self-describing. The SPEC §9.1 acceptance
  gate must continue to use `DEFAULT_SEED`.

### Changed

- **ProvBench labeler — Phase 0b hardening pass 3 (2026-05-12).**
  Four labeling-correctness clusters fixed; SPEC v1 is unchanged:
  (A) visibility narrowing (`pub(crate)` / `pub(super)` / `pub(in path)` /
  private) is now classified as `StaleSourceChanged` per SPEC §5 rule
  ordering rather than `NeedsRevalidation`;
  (B) replay symbol resolution is commit-tree-local — `CommitSymbolIndex`
  built via tree-sitter per commit, eliminating the runtime RA dependency
  (RA tooling pin and `tests/replay_ra.rs` retained for future cross-crate
  / macro-expanded work);
  (C) rename detection requires a typed `RenameCandidate` with matching
  `kind` + `container` and a T₀-presence check to prevent false positives
  from pre-existing same-named symbols;
  (D) doc-claim matching is relocation-tolerant — post-state lookup uses
  `qualified_name` rather than byte-offset hash so claims that move lines
  are still matched correctly.
- **ProvBench labeler — Phase 0b hardening pass 2 (2026-05-09).**
  Deterministic `fact_id`s via pure-string path normalization (no
  `pwd`-sensitive canonicalization), fail-closed behavior on
  rust-analyzer indexing timeout, explicit invalid-UTF-8 error in the
  doc-claim extractor (no more silent zero-fact corpus on a corrupted
  README), structured CSV via the `csv` crate for the spot-check sample,
  and pinned `linux-x86_64` tooling hashes for the `ubuntu-latest` GitHub
  runner so CI matches the canonical `aarch64-darwin` freeze
  environment.
- **Breaking (wire):** MCP tool ids dropped the `ironmem_` prefix now that the server id itself is `ironmem`. For example, `ironmem_search` → `search`, `ironmem_collab_start` → `collab_start`. Clients invoking tools as `mcp__ironmem__ironmem_*` must update to `mcp__ironmem__*`.
- Renamed workspace crate `ironrace-memory` → `ironmem` and MCP server id → `ironmem`. The on-disk data directory `~/.ironrace-memory/` is preserved for user-data backcompat.

## [0.1.0] - 2026-04-15

### Added

- MCP server (`ironmem serve`) with JSON-RPC 2.0 over stdio
- Semantic search via HNSW index (all-MiniLM-L6-v2 ONNX embeddings, 384-dim)
- Knowledge graph with temporal triples — add, query, invalidate, timeline
- Memory graph traversal — BFS, tunnel detection, graph stats
- Diary read/write with wing-scoped entries
- Drawer CRUD — add, delete, list wings/rooms, full taxonomy
- Incremental workspace mining (`ironmem mine`) with SHA-256 manifest cache
- ChromaDB/mempalace migration (`ironmem migrate --from <path>`)
- Auto-bootstrap on first `serve` or `hook` — migrate-or-init + initial mine; disable with `IRONMEM_AUTO_BOOTSTRAP=0`
- `IRONMEM_WORKSPACE_ROOT` to pin the auto-mine target without passing it on the command line
- `IRONMEM_MIGRATE_FROM` to point migration at a custom ChromaDB store path
- `IRONMEM_DB_PATH`, `IRONMEM_MODEL_DIR`, `IRONMEM_MCP_MODE` for runtime config overrides
- Hook support for Claude Code and Codex: `session-start`, `stop`, `precompact`
- Three MCP access modes: `trusted`, `read-only`, `restricted`
- Input sanitization and content length limits on all write paths
- WAL audit log with automatic 30-day pruning
- SHA-256 checksum verification on model download
- Plugin packaging for Claude Code (`.claude-plugin/`)
- Plugin packaging for Codex (`.codex-plugin/`)
- Memory protocol guidance returned from `ironmem_status` and surfaced in plugin `defaultPrompt`
- Non-blocking startup: DB opens in Phase 1 (<50 ms); ONNX model loads in a background thread with `warming_up` status flag
- Embedder hot-swap on first tool call after background init completes
- `IRONMEM_EMBED_MODE=noop` for smoke tests and CI without the ONNX model
- `IRONMEM_DISABLE_MIGRATION=1` to skip first-run mempalace migration
- Stale `bootstrap.lock` auto-recovery on startup
- MCP smoke test script (`scripts/mcp_smoke_test.py`)
- Tag-triggered release workflow with macOS and Linux binary archives
- Integration tests: MCP protocol contract, plugin metadata validation, mining end-to-end, bootstrap races, migration corruption/idempotency

### Changed

- Search overfetch increased from 3x to 5x (minimum 30 candidates)
- Mining skips hidden files and directories by default; set `IRONMEM_MINE_HIDDEN=1` to index dot-paths
- Bootstrap no longer infers workspace from `cwd`; explicit roots required for auto-mining
- `serve` fails closed on bootstrap errors instead of starting with partial initialization
- Re-mining replaces a file's drawers transactionally after embeddings are computed
- Migration from ChromaDB imports drawers and knowledge-graph data transactionally
- Hook session summaries land in the same diary stream as normal diary writes

### Fixed

- Sanitized `cwd` and `transcript_path` values before hook diary persistence
- Rejected system directory prefixes for mining and migration inputs
- Removed `.env` from the mining allowlist
- Added bounded SQLite busy retries during startup schema work
- Serialized env-var-mutating bootstrap tests to prevent race conditions

### Removed

- `properties` field from the `entities` table and `Entity` struct
