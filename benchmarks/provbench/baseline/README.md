# ProvBench Phase 0c — LLM-as-invalidator baseline

> **Status:** Phase 0c baseline runner. Frozen contract: `../SPEC.md` (FROZEN 2026-05-09).
> **Benchmark scaffolding only.** This crate is excluded from the `ironrace-memory`
> Cargo workspace and is never imported by any ironmem crate. No ironmem code path
> calls the Anthropic API at runtime.

## Operational vs spec budget

| Cap | Value | Source |
|---|---|---|
| Spec ceiling (immutable) | $250 | SPEC §6.2 / §15 |
| Operational guardrail (default) | $25 | This crate; configurable via `--budget-usd` |

Pre-flight refuses to start if the manifest's schema-derived worst-case cost
exceeds the operational cap. Live meter aborts at 95% of the operational cap.
The spec ceiling is asserted as a hard cap that can never be exceeded.

## The three subcommands

```bash
# 1. Sample a stratified subset (deterministic, seed-pinned)
cargo run --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- sample \
  --corpus    benchmarks/provbench/corpus/ripgrep-af6b6c54-c2d3b7b.jsonl \
  --facts     benchmarks/provbench/facts/ripgrep-af6b6c54-<labeler-sha>.facts.jsonl \
  --diffs-dir benchmarks/provbench/facts/ripgrep-af6b6c54-<labeler-sha>.diffs/ \
  --out       benchmarks/provbench/results/phase0c/<run-id>/manifest.json

# 2. Score the manifest against Sonnet 4.6 (atomic checkpointing; --resume supported)
cargo run --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- run \
  --manifest benchmarks/provbench/results/phase0c/<run-id>/manifest.json \
  [--max-batches 10]    # canary

# 3. Compute metrics over the completed run
cargo run --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- score \
  --run benchmarks/provbench/results/phase0c/<run-id>
```

### Live canary (≤ $1)

Before a full run, confirm the API path works end-to-end and that prompt
caching is engaging:

```bash
export ANTHROPIC_API_KEY=...    # or IRONMEM_ANTHROPIC_API_KEY
cargo run --release --manifest-path benchmarks/provbench/baseline/Cargo.toml -- run \
  --manifest benchmarks/provbench/results/phase0c/<run-id>/manifest.json \
  --max-batches 10
```

Look for non-zero `cache_read_input_tokens` in `run_meta.json` once any commit
has ≥2 batches.

## Coverage honesty (§9.2)

A subset run records `"coverage": "subset"` in `metrics.json` and does **not**
claim the full SPEC §9.2 acceptance gate is satisfied. Full-corpus coverage
remains a future step, blocked on either a higher operational cap or a tighter
cost model.

## Dev / fixture mode

`--dry-run` runs the loop with fabricated `"valid"` decisions (zero cost, no
network). `--fixture-mode <dir>` reads canned API responses from
`<dir>/<batch_id>.json` keyed by deterministic batch id (`<commit_sha>-<batch_index>`).
Both modes are for development; never use them to generate SPEC §9.2 artifacts.

## Reproducibility

- Sample manifest pinned by `--seed` (default `0xC0DEBABEDEADBEEF`, same as the labeler).
- `manifest.content_hash` covers every field except itself; `--resume` verifies it.
- `spec_freeze_hash` and `labeler_git_sha` recorded in every artifact.
- Per-prediction `request_id` traces back to specific API responses.

## Build + test

```
cargo build  --release --manifest-path benchmarks/provbench/baseline/Cargo.toml
cargo test   --release --manifest-path benchmarks/provbench/baseline/Cargo.toml
cargo fmt    --manifest-path benchmarks/provbench/baseline/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/provbench/baseline/Cargo.toml --all-targets -- -D warnings
```
