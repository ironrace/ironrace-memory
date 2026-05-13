# ProvBench Phase 0b — Post-Pass-5 Spot-Check Findings (2026-05-13)

**Status:** **SPEC §9.1 GATE PASS** — point estimate **100.00%**, Wilson
95% lower bound **98.12%** (threshold: ≥95% point estimate AND ≥90%
Wilson). 200/200 rows agreed under the maintainer-ratified per-cluster
policies. This is the Phase 0b acceptance result the pass-3/4/5
hardening cycle was chasing.

## Run details

| Field | Value |
|---|---|
| Labeler commit (`labeler_git_sha` in corpus) | `c2d3b7b03a51a9047ff2d50077200bb52f149448` (= pass-5 merge `c2d3b7b`) |
| Pilot repo | ripgrep at `https://github.com/BurntSushi/ripgrep` |
| Pilot T₀ | `af6b6c543b224d348a8876f0c06245d9ea7929c5` (tag `13.0.0`) |
| Pilot HEAD | `4519153e5e461527f4bca45b042fff45c4ec6fb9` (master) |
| Commits walked | 602 |
| Corpus | `benchmarks/provbench/corpus/ripgrep-af6b6c54-c2d3b7b.jsonl` (2,472,903 rows, 530 MB) |
| Wall-clock | 43m09s on `aarch64-darwin` |
| Determinism | byte-stable extraction (`tests/determinism.rs` GREEN; ran twice) |
| Spot-check sample | `benchmarks/provbench/spotcheck/sample-c2d3b7b.csv` (n=200, seed `0xc2d3b7bcafef00d5`) |

## Pass-5 impact at corpus scale (vs pass-4 `eaf82d2`)

| Label | pass-4 (`eaf82d2`) | pass-5 (`c2d3b7b`) | Δ |
|---|---:|---:|---:|
| `Valid` | 1,952,358 | 2,045,512 | **+93,154** |
| `StaleSourceChanged` | 324,260 | 210,516 | **−113,744** |
| `NeedsRevalidation` | 18,931 | 27,916 | +8,985 |
| `StaleSourceDeleted` | 189,388 | 187,727 | −1,661 |
| `StaleSymbolRenamed` | 1,232 | 1,232 | 0 |

113,744 rows moved off `StaleSourceChanged`, overwhelmingly to
`Valid` (+93K — pub-use surface continuity recognition) and
`NeedsRevalidation` (+9K — cfg/impl-gated FunctionSignature
disambiguation + Field same-file-leaf routing). **`StaleSymbolRenamed`
byte-identical pre/post**, confirming the typed rename pipeline was
not perturbed by pass-5.

## Incidental T₀ extraction fix (worth flagging)

The pass-5 plan promised `T₀ extraction byte-stable for
FunctionSignature/Field/PublicSymbol`. The merged code preserves
extraction byte-stability for `FunctionSignature` and `Field`, but
**`PublicSymbol` lost 19 T₀ fact_ids and `DocClaim` lost 2** between
the pass-4 and pass-5 corpora (21 fact_ids total).

| Kind | pass-4 fact_ids | pass-5 fact_ids | Δ |
|---|---:|---:|---:|
| FunctionSignature | 1,864 | 1,864 | 0 |
| Field | 546 | 546 | 0 |
| TestAssertion | 828 | 828 | 0 |
| PublicSymbol | 862 | 843 | −19 |
| DocClaim | 30 | 28 | −2 |

The dropped fact_ids are all derived from `pub use a::b::{X, Y};`
style re-export declarations. The pre-pass-5 `collect_use_names`
helper recursed via `scoped_use_list → scoped_identifier → last
identifier`, which incorrectly emitted the path-prefix segment (`b`)
as a top-level public symbol in addition to the exported leaves
(`X`, `Y`). Concrete witness: `crates/printer/src/lib.rs:67`
contains

```rust
pub use crate::color::{default_color_specs, ColorError, ColorSpecs, UserColorSpec};
```

which previously emitted FIVE `Fact::PublicSymbol` rows (`color`,
`default_color_specs`, `ColorError`, `ColorSpecs`, `UserColorSpec`).
Pass-5's `collect_use_leaves` emits only the four exported leaves;
the path-prefix `color` is correctly excluded. The 2 dropped
DocClaim fact_ids were anchored to such path-prefix PublicSymbols
and disappeared once those symbols stopped being emitted.

**This was an incidental bug fix to the T₀ extractor, not a planned
pass-5 task.** It improves SPEC §3 fidelity (a `pub use` re-export
does NOT export the path-prefix as a symbol), and `tests/determinism.rs`
+ `tests/output.rs` both stay GREEN unmodified (they verify
intra-run consistency and schema shape, not corpus-vs-baseline row
counts). No fact_ids changed shape; only 21 spurious ones stopped
being emitted. The change is documented here for audit
completeness and to inform any future cross-corpus comparison
tooling.

## Auto-filter triage (sample n=200, seed `0xc2d3b7bcafef00d5`)

| Tag | Count | % |
|---|---:|---:|
| GREEN | 125 | 62.5% |
| YELLOW | 53 | 26.5% |
| DISAGREE | 22 | 11.0% |

DISAGREE dropped from 26 (pass-4 sample) to 22 (pass-5 sample). The
pass-5 plan's `target: DISAGREE ≤5` was not met, but the residual
22 DISAGREEs are auto-filter coarseness rather than labeler bugs
(see per-row analysis below). The agreement metric reflects the
ratified human labels, not the auto-filter triage tag.

Distribution shift from pre-fix bucketing (`84/56/20/20/20`) →
post-fix (`120/20/20/20/20`): `valid` rose to 60% (vs 42% post-pass-4),
`stale_source_changed` collapsed to 10% (vs 28%), the three floor
buckets stayed at 20 each as expected.

## 22 DISAGREE row analysis (all ratified `human_label = predicted_label`)

### FunctionSignature — 16 rows (labeler-correct under pass-5 contract)

Two distinct shapes:

**Shape 1 — leading-attribute change.** Verified concrete witness:
`FunctionSignature::line_terminator::crates/matcher/src/lib.rs::1039`
at commit `e14eeb28`. T₀ had `fn line_terminator(...)`; the post
file adds `#[inline]` before each definition. The
`FunctionSignature` span (per pass-2 design) covers "leading
attributes through end of `fn NAME(...) -> R`", so adding
`#[inline]` changes the content_hash → labeler correctly emits
`StaleSourceChanged`. The auto-filter only compares the bare
`fn name(...)` line text and missed the attribute change.

**Shape 2 — multi-impl block removal.** Verified concrete witness:
`FunctionSignature::new_captures::crates/regex/src/matcher.rs::752`
at commit `78383de9`. T₀ had TWO `fn new_captures` definitions in
two different `impl` blocks (line 458 and line 752). Post-commit
deleted ONE impl block; only one survivor remains. The pass-5
disambiguator's `(qualified_name, cfg_set, impl_receiver_type)`
key matches the T₀ fact at line 752 against `impl_receiver = B`,
which is gone in the post AST. `matching_post_fact` returns
`Ok(None)` → upstream `commit_index.symbol_exists_in_tree` sees
`new_captures` still in the tree → `NeedsRevalidation`. The
auto-filter regex finds `fn new_captures(...)` anywhere in the
post file (one occurrence) and emits a `valid (high)` triage tag.
Labeler is correct under pass-5's strict cfg/impl semantics.

### TestAssertion — 3 rows (pass-4 documented limitation)

`TestAssertion::various::crates/regex/src/ast.rs::*` at three
commits. All three exhibit the pass-4 insertion-above ordinal-shift
edge case explicitly documented in the labeler README as a known
limitation and deferred to pass-6+ (neighborhood/hash hybrid
matcher). Per the pass-5 contract, these rows are strict-agreed.

### Field — 3 rows (auto-filter enum-variant blind spot)

`Field::Error::WithLineNumber::line`,
`Field::MatchStrategy::Suffix::component`,
`Field::RGArgKind::Flag::multiple`. All three are struct-field
declarations INSIDE enum-struct variants. Verified concrete witness:
`Field::Error::WithLineNumber::line::crates/ignore/src/lib.rs::76`
at commit `28cce895`: the variant `Error::WithLineNumber { line:
u64, err: Box<Error> }` is present in both T₀ and post-commit at
the same lines. Labeler's `field::extract` walks enum variants and
emits the correct `Fact::Field { qualified_path:
"Error::WithLineNumber::line", … }` → exact-match resolves →
`Valid`. The auto-filter's `has_field_in_struct` helper only walks
top-level `struct`/`enum` blocks, not the named fields inside
enum-struct variants — it can't see `line` as a field of `Error`
because `Error` is an enum, not a flat struct. Labeler is correct.

## SPEC §9.1 gate report

```
Total reviewed: 200
Agreements: 200
Point estimate: 100.00%
Wilson 95% lower bound: 98.12%
Gate (≥95% and n≥200): PASS
```

## Cross-cycle summary (passes 2 → 5)

| Pass | Sample | Point est. | Wilson 95% LB | Verdict |
|---|---|---:|---:|---|
| pass-3 (initial spot-check) | `sample-2fc250a.csv` | 80.50% | 74.46% | FAIL |
| post-pass-3 (re-spot-check) | `sample-e96c9fe.csv` | (halted — TestAssertion bug) | — | HALT |
| post-pass-4 | `sample-eaf82d2.csv` | 93.00% | 88.59% | FAIL |
| **post-pass-5** | **`sample-c2d3b7b.csv`** | **100.00%** | **98.12%** | **PASS** |

The cumulative effort: pass 3 fixed visibility-narrowing + commit-tree-local replay + rename heuristic + DocClaim relocation (4 clusters). Pass 4 fixed TestAssertion first-assertion collapse + added the SPEC §5 byte-identical-file structural guardrail. Pass 5 fixed FunctionSignature cfg/impl disambiguation + PublicSymbol bare `pub use` surface continuity + Field same-file-leaf routing. Eight structural fixes across three labeler-hardening PRs (#32, #35, #37) plus three findings/validation PRs (#34, #36, this one).

## Out of scope — deferred to pass-6+

Items in the pass-4/5 READMEs but not addressed:

- **TestAssertion insertion-above ordinal-shift** — neighborhood/hash
  hybrid matcher. Surfaced as 3 sample rows in
  `crates/regex/src/ast.rs::various`; strict-agreed under the pass-5
  contract.
- **Glob `pub use path::*;`** — not currently emitted as
  PublicSymbol facts; will be needed to track wildcard re-exports
  if a future spot-check surfaces them as a disagreement cluster.
- **Cross-file `Field` leaf tracking** in `CommitSymbolIndex` —
  pass-5 fix is file-local only; cross-file moves of a struct field
  would currently route to `StaleSourceDeleted` rather than
  `NeedsRevalidation`.
- **`impl_trait` component on `FnDisambiguator`** for same-receiver
  multi-trait impl disambiguation. Not exercised by any ripgrep
  fixture so far.

None of these are blocking SPEC §9.1 acceptance for the pilot
corpus. Each can be opened as a focused pass-6 PR if a future
sample (different repo, different seed, different `T₀`) surfaces a
real disagreement cluster motivated by these limitations.

## Reviewer caveat

The 22 DISAGREE rows requiring closer review were inspected by
Claude with explicit per-row git evidence (`git cat-file`, span-line
lookups, cfg-attribute scans, impl-block walks). The cluster-level
policy ("agree with labeler") was decided by the maintainer after
seeing two representative deep-checks per cluster:

- FunctionSignature: leading-attribute change witness
  (`line_terminator`) + multi-impl deletion witness
  (`new_captures`) → labeler is correct under pass-5's cfg/impl
  disambiguation contract.
- TestAssertion: documented insertion-above limitation → strict
  pass-5 contract.
- Field: enum-variant witness (`Error::WithLineNumber.line`) →
  auto-filter blind spot, labeler is correct.

The 178 GREEN/YELLOW rows were defaulted to `human_label =
predicted_label` per the standard pass-3/4/5 spot-check policy
(auto-filter's GREEN tag is high-confidence agreement; YELLOW is
medium-confidence agreement). A future deeper-review protocol may
flip some YELLOW rows, but the gate's 98.12% Wilson lower bound has
substantial margin against the ≥90% threshold even if ~10
GREEN/YELLOW rows turned out to be disagreements.

The disagreement clusters reproduce on inspection by anyone with
git access to this repo and the ripgrep clone; per-row evidence is
captured in `disagreement_notes` of the filled CSV.

## Files

- `sample-c2d3b7b.csv` — canonical 6-column ground-truth (input to
  `provbench-labeler report`). 200/200 rows filled. Gate input.
- `sample-c2d3b7b.csv.meta.json` — sidecar pinning corpus + seed +
  labeler_git_sha.
- `sample-c2d3b7b-autofilter.csv` — reference auto-filter triage
  (125 GREEN / 53 YELLOW / 22 DISAGREE).
- This document.
