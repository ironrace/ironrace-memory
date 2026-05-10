# ProvBench Phase 0b Labeler

Mechanical labeler for the ProvBench-CodeContext pilot corpus.
**Frozen contract:** `../SPEC.md`. This crate is excluded from the
ironrace-memory workspace because Phase 0 must be releasable as a
standalone reproducible artifact.

## Reproducing the pilot corpus

1. **Verify tooling pins.**
   ```
   cargo run --manifest-path benchmarks/provbench/labeler/Cargo.toml -- verify-tooling
   ```
   The expected hashes are SPEC §13.1. A mismatch is fatal; do not
   attempt to "work around" it.

2. **Clone the pilot at T₀.**
   ```
   mkdir -p benchmarks/provbench/work
   git clone https://github.com/BurntSushi/ripgrep \
     benchmarks/provbench/work/ripgrep
   git -C benchmarks/provbench/work/ripgrep checkout \
     af6b6c543b224d348a8876f0c06245d9ea7929c5
   ```

3. **Run the labeler.**
   ```
   cargo run --release --manifest-path benchmarks/provbench/labeler/Cargo.toml -- run \
     --repo benchmarks/provbench/work/ripgrep \
     --t0 af6b6c543b224d348a8876f0c06245d9ea7929c5 \
     --out benchmarks/provbench/corpus/ripgrep-af6b6c54-$(git rev-parse --short HEAD).jsonl
   ```
   Output is JSONL, one `fact_at_commit` row per line, sorted
   `(fact_id, commit_sha)`. Every row carries `labeler_git_sha` matching
   the labeler commit at run time.

4. **Determinism check.**
   ```
   cargo test --release --manifest-path benchmarks/provbench/labeler/Cargo.toml --test determinism
   ```
   And on the real corpus:
   ```
   <re-run step 3 with --out file2>
   diff <file1> <file2>
   ```
   The diff must be empty.

5. **Spot-check sampling.**
   ```
   cargo run --manifest-path benchmarks/provbench/labeler/Cargo.toml -- spotcheck \
     --corpus benchmarks/provbench/corpus/<file>.jsonl \
     --out benchmarks/provbench/spotcheck/sample-$(git rev-parse --short HEAD).csv
   ```
   The CSV columns are: `fact_id,commit_sha,bucket,predicted_label,human_label,disagreement_notes`.
   Open the CSV and fill the `human_label` column for each of the 200
   rows. Save and run:
   ```
   cargo run --manifest-path benchmarks/provbench/labeler/Cargo.toml -- report \
     --csv benchmarks/provbench/spotcheck/sample-<sha>.csv
   ```
   The report prints the point estimate and Wilson 95% lower bound, and
   appends a Markdown summary under `benchmarks/provbench/spotcheck/`.

## Acceptance gate (SPEC §9.1)

Phase 0b is accepted iff:
- The point estimate of spot-check agreement is **≥95%**.
- The Wilson 95% lower bound is reported (informational; not part of
  the gate but required by §9.1).
- The determinism check is byte-identical.

## Limitations

- v1 supports Rust only. The held-out Python repo (`flask`) is **not**
  exercised by this labeler. A `tree-sitter`-based Python path is
  future work and **not** required for Phase 0b acceptance.
- `rust-analyzer` is invoked over LSP stdio per commit. Wall-clock cost
  scales with commit count; the pilot run on ripgrep at T₀ → T₀+~600
  commits is expected to take 30–90 minutes on an M-series Mac.
