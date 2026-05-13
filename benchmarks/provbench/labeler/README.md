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

## Behavior

- **Visibility narrowing classified as source changed.** Public symbols
  whose visibility is narrowed to `pub(crate)`, `pub(super)`,
  `pub(in path)`, or private are classified as `StaleSourceChanged` rather
  than `NeedsRevalidation`. Per SPEC §5 rule ordering, a narrowing
  represents a semantic change to the public API surface and is therefore
  treated as a source change, not merely a revalidation trigger.

- **Replay symbol resolution is commit-tree-local.** For each commit a
  `CommitSymbolIndex` is built from that commit's `.rs` tree (via
  tree-sitter) before any fact is classified; `rust-analyzer` is no longer
  consulted at replay time. This eliminates the runtime RA dependency for the
  hot classification path. Live RA tooling and the `PINNED_BINARIES`
  tooling-pin table (in `src/tooling.rs`) remain in the crate for
  `tests/replay_ra.rs` (pinned-binary test) and for future cross-crate /
  macro-expanded work. Files added after T₀ are included in the per-commit
  index, so same-qualified symbols moved into new files route to
  `NeedsRevalidation` rather than `StaleSourceDeleted`.

- **Rename detection requires AST context match.** A `RenameCandidate` is a
  typed struct carrying `container` (the enclosing module or impl block)
  along with `qualified_name`, `leaf_name`, and `span`. Same-kind filtering
  is enforced upstream by `rename_candidates_for_typed`, which only collects
  candidates from the original fact's kind. A candidate is only promoted to
  a rename when the container matches the original symbol *and* the
  candidate was not already present at T₀ — the structural T₀-presence
  check prevents false positives from pre-existing symbols with the same
  name.

- **DocClaim matching is relocation-tolerant.** The post-state doc-claim
  lookup searches by `qualified_name` rather than by byte-offset hash.
  A claim that moves to a different line or section in a later commit is
  still matched correctly as long as the qualified name is preserved.

The labeler is **fail-closed** by design. Silently producing labels in any
of the following situations would corrupt the corpus, so each surfaces as
an error and aborts the run:

- **Tooling-pin mismatch.** `verify-tooling` calls
  `tooling::resolve_from_env()`, which hard-fails when a binary on `PATH`
  (or at the documented fallback) does not match the SHA-256 in
  `PINNED_BINARIES` for the current platform. Distros patch — version
  strings are not enough. Replay itself no longer calls rust-analyzer.
- **Invalid UTF-8 in markdown.** The doc-claim extractor refuses to
  silently produce zero facts on a corrupted README; it returns `Err`
  with the offending file path so reviewers can locate the bad blob.
- **Malformed git SHA.** `validate_sha_hex` rejects anything that isn't
  exactly 40 lowercase hex characters before that string is passed to
  `git ls-tree`, so a malformed value cannot reach a subprocess argv.
- **Cross-platform `fact_id`s.** Source paths are normalized to forward
  slashes via a pure-string transform (`normalize_path_for_fact_id`)
  before they enter a `fact_id`. The single `Path::canonicalize` call in
  the labeler runs once, on the repo root, in `Pilot::open`. No absolute
  filesystem path can leak into a `fact_id` regardless of `pwd`.

The spot-check CSV is written via the `csv` crate, not hand-rolled
formatting, so a stray comma or newline in a `disagreement_notes` cell
round-trips through reader/writer correctly.

## Limitations

- v1 supports Rust only. The held-out Python repo (`flask`) is **not**
  exercised by this labeler. A `tree-sitter`-based Python path is
  future work and **not** required for Phase 0b acceptance.
- Per-commit classification uses a tree-sitter-built `CommitSymbolIndex`
  rather than a live `rust-analyzer` query. This loses semantic resolution
  for cross-crate references and macro-expanded symbols. The pinned RA
  binary is still covered by `verify-tooling` and `tests/replay_ra.rs` for
  future work that needs deeper semantic resolution.

## Reproducibility / supported platforms

The pinned-binary table in `src/tooling.rs` covers exactly two platforms:

| Platform                  | rust-analyzer install path                                   | tree-sitter install path                              |
|---------------------------|--------------------------------------------------------------|-------------------------------------------------------|
| `aarch64-darwin` (macOS)  | rustup `stable-aarch64-apple-darwin` component               | Homebrew (`brew install tree-sitter`)                 |
| `x86_64-linux-gnu` (CI)   | Decompressed `rust-analyzer-x86_64-unknown-linux-gnu.gz`     | Decompressed `tree-sitter-linux-x64.gz`               |

For `x86_64-linux-gnu` (the `ubuntu-latest` GitHub runner), the pinned
hashes correspond to the **decompressed** binaries published as `.gz`
artifacts on each tool's GitHub release. CI must install the tools by
downloading the upstream `.gz`, gunzipping, and placing the resulting
binary on `PATH` (or at the documented fallback under `/usr/local/bin/`).
Installs via `apt`, `snap`, or `rustup` may produce different on-disk
bytes and **will fail hash verification**.

Upstream artifact URLs for the Linux pin:

- `rust-analyzer` 1.85.0:
  `https://github.com/rust-lang/rust-analyzer/releases/download/2025-02-17/rust-analyzer-x86_64-unknown-linux-gnu.gz`
- `tree-sitter` 0.25.6:
  `https://github.com/tree-sitter/tree-sitter/releases/download/v0.25.6/tree-sitter-linux-x64.gz`

`x86_64-darwin` (Intel Mac) and `aarch64-linux` (e.g. ARM CI runners)
are explicitly **out of scope** for this hardening pass. Adding them
requires running `shasum -a 256` against the decompressed upstream
artifact and committing the result to `PINNED_BINARIES` — never copy a
hash from a secondary source.

Phase 0b tooling verification remains valid only on a supported
platform. `resolve_from_env()` hard-fails on any other host.
