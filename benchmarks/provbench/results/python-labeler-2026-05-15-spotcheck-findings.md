# Python labeler — SPEC §9.1 spot-check (PENDING HUMAN REVIEW)

**Date generated:** 2026-05-15
**Corpus:** `benchmarks/provbench/corpus/flask-2f0c62f5-fba84cd.jsonl`
**Sample:** `benchmarks/provbench/results/python-labeler-2026-05-15-spotcheck.csv` (200 rows)
**Seed:** `0xC0DEBABEDEADBEEF` (decimal `13897750829054410479`)
**Held-out repo:** `pallets/flask @ 2f0c62f5e6e290843f03c1fa70817c7a3c7fd661` (T₀ = `2.0.0`)
**Labeler git SHA:** `fba84cd` (current `feat/provbench-v1.2b-python-labeler` HEAD before review)
**Corpus size:** 2,265 rows total; 200 sampled per SPEC §9.1

## Status: PENDING

The CSV at `python-labeler-2026-05-15-spotcheck.csv` has the `human_label`
column pre-filled with the labeler's predicted value as a default. The
reviewer must inspect each row against the source file at the recorded
commit and flip `human_label` to the correct value when the prediction
is wrong, plus add a short note in `disagreement_notes`.

When review is complete, run:
```bash
benchmarks/provbench/labeler/target/release/provbench-labeler report \
  --csv benchmarks/provbench/results/python-labeler-2026-05-15-spotcheck.csv
```

This prints the agreement rate + Wilson 95% confidence interval.

## SPEC §9.1 acceptance gate

- **Threshold:** Wilson 95% lower bound ≥ 0.95
- **Verdict:** TBD (pending review)

## Methodology notes

- The Python labeler emits four Fact kinds for flask: `FunctionSignature`,
  `Field`, `PublicSymbol`, `TestAssertion`. `DocClaim` is intentionally
  deferred (see `src/facts/python/doc_claim.rs` for the rationale).
- The stratified sampler uses `bucket` derived from `label` field; for a
  Plan A spot-check on a SINGLE-COMMIT (T₀-only) corpus, every row has
  `label = "valid"` so the sampler degenerates to a simple seeded random
  draw. Stratification has no effect at this stage — it will when the
  held-out evaluation (Plan B) adds stale-class rows.
- Known coverage limitations (also recorded in commit messages):
  1. `__init__.py` collapse not implemented — flask's sparse `__init__.py`
     re-exports (`from .app import Flask`) won't fully resolve through
     `PythonResolver`. Symbol-resolution-dependent rules (R7) will
     under-fire on Python.
  2. Multi-hop import chains capped at one hop.
  3. Relative imports (`from . import X`) punted.
  4. Star imports (`from X import *`) skipped unless `__all__` is defined
     (and we don't currently parse `__all__`).
- Python `DocClaim` extractor is a stub. R5 (`stale_doc_drift`) will not
  fire on Python rows.

## On §9.1 miss

If Wilson 95% LB < 0.95: STOP. Do not weaken the threshold to make the
labeler pass; that is SPEC §10 leakage. Triage by fact kind in the CSV
(group disagreements by `fact_id` prefix — `Field::`, `FunctionSignature::`,
`PublicSymbol::`, `TestAssertion::`) and either:
- Tighten the offending extractor(s); OR
- Drop the offending fact kind for Python and re-run the spot-check
  against a fresh corpus.

In either case: a new sample seed is required after retuning to satisfy
the §10 anti-leakage contract.

## Decision (fill in after review)

- [ ] PASS — labeler accepted at SHA `fba84cd` for Plan B held-out
      evaluation
- [ ] FAIL — see triage notes below

### Triage notes (if FAIL)

Per-fact-kind agreement rates:

| Kind | Predicted | Correct | Wrong | Agreement |
|---|---|---|---|---|
| FunctionSignature | | | | |
| Field | | | | |
| PublicSymbol | | | | |
| TestAssertion | | | | |
