# ProvBench Phase 0b — Post-Pass-4 Spot-Check Findings (2026-05-13)

**Status:** SPEC §9.1 gate **FAIL** — point estimate 93.00%, Wilson 95%
lower bound 88.59% (need ≥95% point estimate AND ≥90% Wilson per the
pass-3 plan). Pass-4 structural fixes worked as designed and unblocked
~14.8% of the corpus, but a residual `FunctionSignature` cluster of the
same shape as pass-4's `TestAssertion` bug holds the gate down. Pass-5
is well-scoped and small.

## Run details

| Field | Value |
|---|---|
| Labeler commit (`labeler_git_sha` in corpus) | `eaf82d2837101810c03dfc3e12bb334b2b07718d` (= pass-4 merge `eaf82d2`) |
| Pilot repo | ripgrep at `https://github.com/BurntSushi/ripgrep` |
| Pilot T₀ | `af6b6c543b224d348a8876f0c06245d9ea7929c5` (tag `13.0.0`) |
| Pilot HEAD | `4519153e5e461527f4bca45b042fff45c4ec6fb9` (master) |
| Commits walked | 602 |
| Output JSONL | `benchmarks/provbench/corpus/ripgrep-af6b6c54-eaf82d2.jsonl` (2,486,169 rows, 534 MB) |
| Wall-clock | 40m46s on aarch64-darwin |
| Determinism | byte-stable extraction (no `Fact`/`fact_id` change in pass-4) |
| Spot-check sample | `benchmarks/provbench/spotcheck/sample-eaf82d2.csv` (n=200, seed `0xfeedbeef20260513`) |

## Pass-4 impact at corpus scale (pre vs post)

| Label | `e96c9fe` (pre-fix) | `eaf82d2` (post-fix) | Δ |
|---|---:|---:|---:|
| `Valid` | 1,591,425 | 1,952,358 | **+360,933** |
| `StaleSourceChanged` | 692,153 | 324,260 | **−367,893** |
| `NeedsRevalidation` | 11,971 | 18,931 | +6,960 |
| `StaleSourceDeleted` | 189,388 | 189,388 | 0 |
| `StaleSymbolRenamed` | 1,232 | 1,232 | 0 |

367,893 rows (14.8% of the corpus) flipped off `StaleSourceChanged`,
overwhelmingly to `Valid` (361K) and a smaller `NeedsRevalidation`
shoulder (7K). **`StaleSourceDeleted` and `StaleSymbolRenamed` were
byte-identical**, which is the strongest possible signal that pass-4's
SPEC §5 byte-identical-file fast path did NOT over-mask anything
pass-3 correctly classified as deleted or renamed.

## Auto-filter triage (sample n=200)

| Tag | Count | % |
|---|---:|---:|
| GREEN | 101 | 50.5% |
| YELLOW | 73 | 36.5% |
| DISAGREE | 26 | 13.0% |

DISAGREE missed the pass-4 plan's `≤5` target (the target was set
before auto-filter coarseness was fully understood). Per-row inspection
showed only ~14 of the 26 DISAGREE rows reflect a real labeler issue;
the rest are auto-filter limitations (enum-variant Fields, multi-impl
FunctionSignatures, TestAssertion body-checks).

## Disagreement clusters (14 labeler-vs-human disagreements)

### Cluster E — cfg-gated FunctionSignature multi-def (9 rows; **dominant residual**)

**Same shape as pass-4's Cluster A but for `FunctionSignature`.**
`function_signature::extract` emits one fact per `fn <name>(...)`
definition, including the cfg-gated variants. Multiple variants can
share the same `qualified_name` (the extractor module-qualifies but
does not include the receiver-impl type or cfg context). When the T₀
fact's specific cfg variant is deleted at a post-commit but
same-named survivors exist in other cfg variants,
`match_post::matching_post_fact` for `Fact::FunctionSignature` calls
`find_map(|f| q == *qualified_name)` and returns the first
survivor's `(span, content_hash)`. Hash mismatch with T₀'s deleted
variant routes to `StaleSourceChanged` — but the honest label is
`NeedsRevalidation` or `StaleSourceDeleted` (the specific variant the
T₀ fact pointed at is gone).

Concrete witness: `FunctionSignature::from_entry_os::crates/ignore/src/walk.rs::368`
at commit `92b35a65` — the T₀ fact was the `#[cfg(not(any(windows, unix)))]`
wasm32 placeholder. That commit removed the wasm32 placeholder; the
`#[cfg(windows)]` and `#[cfg(unix)]` variants survive. Labeler returns
the windows variant's span+hash → mismatch → `StaleSourceChanged`.
A reviewer reading SPEC §5 strictly would call this `StaleSourceDeleted`
for the specific variant the fact_id pointed at.

All 9 rows in the sample:

| fact_id | commit | labeler | human |
|---|---|---|---|
| `FunctionSignature::add::crates/globset/src/lib.rs::801` | `e0075232` | stale_source_changed | needs_revalidation |
| `FunctionSignature::default::crates/searcher/src/line_buffer.rs::68` | `51765f2f` | stale_source_changed | needs_revalidation |
| `FunctionSignature::from_entry_os::crates/ignore/src/walk.rs::368` | `92b35a65` | stale_source_changed | needs_revalidation |
| `FunctionSignature::from_path::crates/ignore/src/walk.rs::399` | `81341702` | stale_source_changed | needs_revalidation |
| `FunctionSignature::is_match::crates/globset/src/lib.rs::774` | `1c775f3a` | stale_source_changed | needs_revalidation |
| `FunctionSignature::new::crates/printer/src/json.rs::455` | `fded2a5f` | stale_source_changed | needs_revalidation |
| `FunctionSignature::new::crates/regex/src/matcher.rs::725` | `eab044d8` | stale_source_changed | needs_revalidation |
| `FunctionSignature::try_captures_iter::crates/matcher/src/lib.rs::769` | `5dec4b8e` | stale_source_changed | needs_revalidation |
| `FunctionSignature::try_captures_iter_at::crates/matcher/src/lib.rs::1229` | `163ac157` | stale_source_changed | needs_revalidation |

**Without this cluster the sample agrees at 97.5% / Wilson ≥ ~94% — clean gate pass.**

### Cluster F — Field outside its named container (3 rows)

T₀ fact pointed at `Container.field`. At the post-commit, the field
name still appears in the same file but no longer inside the same
`struct`/`enum` block — likely a restructure that moved the field into
a sub-struct or variant. The labeler treats this as `valid` or
`stale_source_changed` depending on how the file's overall shape
matches; honest human label is `needs_revalidation` (gray area: same
file, same field name, different parent type).

| fact_id | commit | labeler | human |
|---|---|---|---|
| `Field::Config::dfa_size_limit::crates/regex/src/config.rs::34` | `79f5a5a6` | stale_source_changed | needs_revalidation |
| `Field::Match::absolute_offset::crates/printer/src/jsont.rs::49` | `b7df9f8c` | stale_source_changed | needs_revalidation |
| `Field::SinkContext::kind::crates/searcher/src/sink.rs::441` | `65b1b0e3` | valid | needs_revalidation |

### Cluster G — PublicSymbol `pub use` re-export (2 rows)

`PublicSymbol::pattern::crates/cli/src/lib.rs::174` at two different
commits. The post-state has a `pub use … pattern` re-export (still
part of the public surface), but the labeler's bare-pub check in
`symbol_existence::extract` does not currently recognize `pub use` as
"the public symbol is still here". Honest label is `valid`.

| fact_id | commit | labeler | human |
|---|---|---|---|
| `PublicSymbol::pattern::crates/cli/src/lib.rs::174` | `70ae7354` | stale_source_changed | valid |
| `PublicSymbol::pattern::crates/cli/src/lib.rs::174` | `b9de003f` | stale_source_changed | valid |

### Cluster H — TestAssertion ordinal-shift (8 rows; **explicitly out of scope for pass 4**)

`TestAssertion::various::crates/regex/src/ast.rs::*` at multiple
commits. The T₀ assertion bytes survive verbatim in the post file, but
at a different ordinal in the test fn (assertions were inserted above
or reordered). Pass-4's ordinal contract treats "same ordinal,
different bytes" as `StaleSourceChanged` regardless of whether the
original bytes survive at a different ordinal — this is the documented
"insertion-above" limitation in the labeler README. **Strict
interpretation: labeler did exactly what the contract says.**

Per ratification at the start of human review, these 8 rows were
labeled `predicted_label` (strict agreement with the labeler) so the
agreement number reflects only the bug-class disagreements above. A
neighborhood/hash hybrid matcher in pass-5+ would close this gap.

## Suggested labeler fixes (pass-5 input)

In rough priority order:

1. **Cluster E — `FunctionSignature` cfg/impl disambiguation
   (dominant impact).** Mirror pass-4's `TestAssertion` fix for
   `FunctionSignature`:
   - At T₀ extraction, capture the cfg-attribute set and the enclosing
     impl-receiver type (where present) for each `fn` definition;
     store as a private replay-time disambiguator on `ObservedFact`
     (NOT as a new `Fact` field — schema-stable).
   - In `match_post::matching_post_fact`'s `Fact::FunctionSignature`
     arm, pair T₀ → post by `(qualified_name, (cfg_set, impl_type))`
     rather than `qualified_name` alone. When the T₀ disambiguator
     does not match any post fact, return `Ok(None)` and let the
     existing "symbol not found" path classify accordingly
     (`StaleSourceDeleted` if no rename / no other variant survives,
     `NeedsRevalidation` if a same-qualified-name variant exists
     elsewhere). Affects ~9 sample rows. Pure-ordinal fallback as a
     secondary key for same-`(qualified_name, disambiguator)`
     duplicates.

2. **Cluster G — `pub use` re-export recognition in `symbol_existence`.**
   `symbol_existence::extract` and the pass-3 visibility-narrowing
   helper need to accept `pub use path::<name>;` (and re-export aliases
   `pub use path::X as <name>;`) as a still-public surface. Affects
   ~2 sample rows and likely a tail across the full corpus.

3. **Cluster F — Field-inside-enum-variant + intra-file moves.**
   The auto-filter and the labeler both struggle when a field is
   referenced via its container name but the container is an enum
   variant rather than a top-level struct, or when restructuring
   moves a field into a sub-struct. Likely an `field::extract` walk
   that handles enum variants explicitly. Affects ~3 sample rows.

4. **Cluster H — `TestAssertion` neighborhood/hash hybrid matcher
   (insertion-above tolerance).** Augment the ordinal-primary pairing
   with a fallback content-hash search inside the same test fn body,
   so an unchanged assertion that shifted ordinal still classifies
   `valid`. Documented as a pass-5+ scope item in the labeler README;
   ~8 rows in this sample fell under strict-agreement, but a real-world
   reviewer might count them against the labeler.

After fixes (1)+(2), regenerate the corpus, draw a fresh stratified
sample with a new seed, and re-spot-check. Target: point estimate ≥95%,
Wilson 95% lower bound ≥90% per SPEC §9.1.

## SPEC §9.1 gate report

```
Total reviewed: 200
Agreements: 186
Point estimate: 93.00%
Wilson 95% lower bound: 88.59%
Gate (≥95% and n≥200): FAIL
```

## Reviewer caveat

The 26 DISAGREE rows requiring human judgment were reviewed by Claude
with explicit per-row git evidence (`git cat-file`, span-line lookups,
cfg-attribute scans). Ratification policy was decided by the maintainer
in one batch per cluster after seeing the inspection output:

- TestAssertion ordinal-shift cluster (8 rows) → strict agreement with
  the labeler per the pass-4 contract.
- cfg-gated FunctionSignature cluster (9 rows) → human label =
  `needs_revalidation` (concrete pass-5 input).
- PublicSymbol `pattern` cluster (2 rows) → human label = `valid`
  (`pub use` re-export honest).
- Field-out-of-container cluster (3 rows) → human label =
  `needs_revalidation`.
- 3 Field DISAGREE rows where the auto-filter walked the wrong block
  (field actually still in its named container) → human label =
  `valid` (matches labeler).
- 1 TestAssertion row (`various::216`) where the assertion bytes
  genuinely changed → human label = `stale_source_changed` (matches
  labeler).

The disagreement clusters reproduce on inspection by anyone with git
access to this repo and the ripgrep clone; the per-row evidence is
captured in `disagreement_notes` of the filled CSV.

## Files

- `sample-eaf82d2.csv` — canonical 6-column ground-truth (input to
  `provbench-labeler report`). 200 rows, all `human_label` filled.
- `sample-eaf82d2.csv.meta.json` — sidecar pinning corpus + seed +
  labeler_git_sha.
- `sample-eaf82d2-autofilter.csv` — reference CSV widened with
  `auto_tag` + `auto_note`; pre-input to the fill helper.
- `tools/fill_human_labels.py` — auto-filter → canonical-CSV
  conversion used at the start of human review.
- This document.
