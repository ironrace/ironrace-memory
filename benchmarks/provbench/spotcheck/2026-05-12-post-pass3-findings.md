# ProvBench Phase 0b — Post-Pass-3 Spot-Check Findings (2026-05-12)

**Status:** Human-review of the post-merge stratified sample (200 rows,
seed `0x20272a6b8b64004c`, corpus
`ripgrep-af6b6c54-e96c9fe.jsonl`) was **halted before ratification**
because an independent invariant check surfaced a structural labeler
bug that will guarantee a SPEC §9.1 gate failure regardless of how
individual disagreements are adjudicated. The fix belongs in a Phase 0b
hardening **pass 4** PR; once that lands and the corpus is regenerated,
a fresh stratified sample (new seed) should be drawn and reviewed.

## Run details

| Field | Value |
|---|---|
| Labeler commit (`labeler_git_sha` in corpus) | `e96c9fe53f3bcccf601fc26d05bfd62a0a6ca3c9` (= pass-3 merge `e96c9fe`) |
| Pilot repo | ripgrep at `https://github.com/BurntSushi/ripgrep` |
| Pilot T₀ | `af6b6c543b224d348a8876f0c06245d9ea7929c5` (tag `13.0.0`) |
| Pilot HEAD at run time | `4519153e5e461527f4bca45b042fff45c4ec6fb9` (master) |
| Commits walked (T₀ → HEAD) | 602 |
| Output JSONL | `benchmarks/provbench/corpus/ripgrep-af6b6c54-e96c9fe.jsonl` (2,486,169 rows) |
| Sample | `benchmarks/provbench/spotcheck/sample-e96c9fe.csv` (200 rows, seed `0x20272a6b8b64004c`) |
| Sidecar | `benchmarks/provbench/spotcheck/sample-e96c9fe.csv.meta.json` |
| Tooling | rust-analyzer 1.85.0 pin; tree-sitter 0.25.6 pin (unchanged from pass-3) |
| Platform | `aarch64-darwin` |

## Auto-filter triage

`benchmarks/provbench/spotcheck/tools/autofilter.py` (an independent
`git cat-file` + regex re-derivation of each row's expected label, no
labeler code in the loop) emitted the following triage tags against
the 200-row sample:

| Tag | Count | % | Meaning |
|---|---|---|---|
| GREEN | 107 | 53.5% | High-confidence auto-derived label matches `predicted_label`; eligible for fast-track ratification |
| YELLOW | 56 | 28.0% | Auto-derived label matches but with low confidence or partial check |
| DISAGREE | 37 | 18.5% | Auto-derived label clearly differs from `predicted_label` |

The widened CSV with `auto_tag` and `auto_note` columns is at
`sample-e96c9fe-autofilter.csv` (reference only; not consumed by
`provbench-labeler report`).

## Structural finding — TestAssertion match returns first-assertion-only

### Smoking gun

For each row in the sample, run `md5 <git cat-file -p T₀:path>` vs
`md5 <git cat-file -p commit_sha:path>`. By SPEC §5, when the file at
the row's `commit_sha` is byte-identical to the file at T₀, every
fact pointing into that file is structurally `Valid` — nothing about
the file changed.

| Outcome | Count |
|---|---|
| Rows where the underlying file is byte-identical between T₀ and `commit_sha` | 27 |
| Of those, rows predicted `Valid` (correct) | 12 |
| Of those, rows predicted `StaleSourceChanged` (**SPEC violation**) | **15** |

The 15 SPEC-violating rows break down as:

- 14 × `Fact::TestAssertion`
- 1 × `Fact::FunctionSignature` (`is_hidden::crates/ignore/src/pathutil.rs::22` at commit `fc7e6343` — likely a different edge case; called out as a follow-up below)

### Root cause (TestAssertion)

`benchmarks/provbench/labeler/src/replay/match_post.rs:113-126`:

```rust
Fact::TestAssertion { test_fn, .. } => Ok(post_ast.and_then(|ast| {
    test_assertion::extract(ast, path, &[]).find_map(|f| match f {
        Fact::TestAssertion {
            test_fn: q,
            span,
            content_hash,
            ..
        } if q == *test_fn => Some((span, content_hash)),
        Fact::FunctionSignature { .. }
        | Fact::Field { .. }
        | Fact::PublicSymbol { .. }
        | Fact::DocClaim { .. }
        | Fact::TestAssertion { .. } => None,
    })
})),
```

`test_assertion::extract` emits **one fact per `assert!`/`assert_eq!`/
`assert_ne!` invocation** (`facts/test_assertion.rs:100-150`).
A test function with N assertions therefore produces N facts at T₀,
each with its own `(span, content_hash)` from the
distinct macro invocation site.

`find_map(|f| ... if q == *test_fn)` returns the **first** assertion
in iteration order whose `test_fn` matches. For every T₀ fact in a
multi-assertion test fn, `match_post` returns assertion #1's
`(span, content_hash)`. Assertions 2..N at T₀ therefore always
hash-mismatch against assertion #1's hash and route through
`structurally_classifiable() = true` to `Label::StaleSourceChanged`.

The bug is independent of whether the file actually changed; it
misclassifies non-first assertions even in a byte-identical file.

### Blast radius across the full corpus

`/tmp/multi_assert_count.py` (counts distinct (path, test_fn) pairs
and the distinct assertion lines emitted at T₀ for each):

| Quantity | Value |
|---|---|
| Distinct (path, test_fn) pairs | 160 |
| With ≥2 assertions (= test fns affected by the bug) | 120 (75.0%) |
| Total `TestAssertion` fact_ids in the corpus | 827 |
| Non-first-assertion fact_ids (subject to misclassification) | 667 (80.7%) |

This is not an edge case. It is the dominant `TestAssertion` shape in
the pilot corpus.

### Why pass 3 didn't surface it

Pass 3's 4 disagreement clusters (visibility narrowing,
per-commit RA resolution, rename heuristic, DocClaim relocation)
were derived from the pass-2 spot-check sample `sample-2fc250a.csv`.
That sample drew a different stratification of rows; very few
`TestAssertion` rows landed in the disagreement set. The bug existed
during pass 2 and pass 3 but was masked by sampling variance.

The new sample (`sample-e96c9fe.csv`, fresh seed) drew enough
`TestAssertion` rows for the systematic misclassification to be
unambiguous.

## Auto-filter disagreement distribution by fact kind

| Fact kind | YELLOW + DISAGREE rows | Likely cause |
|---|---|---|
| FunctionSignature | 52 | Mostly: auto-filter regex is too loose on multi-impl files (same fn name appears in two impl blocks; auto-filter says byte-identical because at least one occurrence matches T₀). Some genuine signature changes need ratification. Not believed to be a labeler bug. |
| TestAssertion | 34 | Vast majority traceable to the first-assertion-only bug above. Expected to collapse to GREEN once pass 4 ships. |
| PublicSymbol | 5 | Mix of `pub use` re-exports the auto-filter regex still misses + genuine narrowing cases pass-3 already handles. Small enough to ratify individually after pass 4. |
| Field | 2 | `Field::Begin::path` and `Field::SinkContext::absolute_byte_offset` — likely enum variants treated as struct containers by the auto-filter; needs row-by-row ratification post pass 4. |

The 52 `FunctionSignature` rows include 5 where the auto-filter and
the labeler trivially disagree because the file has a single fn
occurrence and is byte-identical (`add_all`, `device_num`,
`is_hidden`, `replace` × 2). One of those (`is_hidden`) is also
inside the 15-row SPEC-violation set, suggesting a separate latent
bug worth investigating in pass 4.

## Suggested pass-4 hardening (input to the plan)

In rough priority order:

1. **TestAssertion match disambiguation.** In `match_post.rs:113-126`,
   match the T₀ fact to the corresponding post-commit fact by both
   `test_fn` AND a structural ordinal (assertion N-th in the test fn
   body) OR by `span` line within the test fn (relative to fn start).
   Either approach restores the one-to-one mapping from T₀ assertions
   to post assertions. The current `find_map(|f| q == *test_fn)`
   silently collapses N facts to one. ~14 rows in the sample flip;
   667 fact_ids across the corpus are at risk.

2. **Byte-identical-file structural invariant.** Add an early-return
   in `Replay::classify` (or the equivalent location upstream of fact
   matching): if `read_blob_at(t0, path) == read_blob_at(commit, path)`
   for the fact's path, classify as `Valid` without consulting the
   per-fact match path. This is a SPEC §5 invariant: an unchanged file
   cannot contain a stale fact. It is also a defensive guardrail
   against future per-fact-matching regressions of the same shape.

3. **Investigate the `is_hidden` FunctionSignature outlier.** The file
   `crates/ignore/src/pathutil.rs` is byte-identical between T₀ and
   commit `fc7e6343`, but `FunctionSignature::is_hidden::…::22` at
   that commit is labeled `StaleSourceChanged`. With (2) above, this
   is masked structurally — but the underlying cause (likely the
   `content_hash` derivation diverging from the AST emit) should be
   pinpointed and fix-tested independently to avoid a hidden
   regression class.

   **Status after pass 4 (2026-05-13):** consciously deferred. The
   pass-4 byte-identical-file fast path (item 2) structurally masks
   the symptom for any unchanged file, including this one. The
   underlying per-fact matcher root cause has NOT been investigated;
   it remains an open follow-up for pass 5 (alongside the
   insertion-above ordinal-shift TestAssertion limitation). A targeted
   pass-5 RED test should reproduce the hash mismatch with the
   fast-path disabled in a test-only configuration so the matcher bug
   can be pinpointed without relying on the guardrail.

After fixes (1)+(2), regenerate
`benchmarks/provbench/corpus/ripgrep-af6b6c54-<labeler-sha>.jsonl`,
draw a fresh stratified sample (new seed) via
`provbench-labeler spotcheck --seed`, and resume human-review. The
existing `sample-e96c9fe.csv` should not be retained as
ground-truth input because its row population was drawn against a
known-buggy labeler.

## Files

- `sample-e96c9fe.csv` — initial post-merge sample (untouched
  `human_label` column); see commit `44d390d` for provenance.
- `sample-e96c9fe.csv.meta.json` — sidecar pinning corpus / seed / `n`
  / labeler git SHA.
- `sample-e96c9fe-autofilter.csv` — reference CSV widened with
  `auto_tag` and `auto_note` columns from
  `tools/autofilter.py`.
- `tools/autofilter.py` — independent re-derivation pipeline.
- This document.

## Reviewer caveat

The TestAssertion bug and its blast-radius numbers were derived from
purely structural git evidence (`md5` of blobs at two commits) and
the labeler's own source. They do not depend on the auto-filter's
correctness. The auto-filter is reference-only and exists to triage
which rows would have needed human eyes had the gate-blocking bug not
been present.
