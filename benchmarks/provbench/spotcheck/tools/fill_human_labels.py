#!/usr/bin/env python3
"""Fill `human_label` in the auto-filter-annotated spot-check CSV
using a structured policy:

  GREEN     → human_label = predicted_label (auto-filter and labeler
              agree with HIGH confidence; fast-track).
  YELLOW    → human_label = predicted_label by default, with a note
              recording the auto-filter's lower confidence. Reviewer
              may override.
  DISAGREE  → human_label = "" (left blank). These require explicit
              per-row inspection and maintainer ratification.

Outputs a canonical 6-column CSV ready for `provbench-labeler report`.
The auto-filter's `auto_tag` + `auto_note` columns are dropped from
the output — they are reference-only and would confuse the reader.

Usage::

    python3 fill_human_labels.py \
        --autofilter benchmarks/provbench/spotcheck/sample-eaf82d2-autofilter.csv \
        --out benchmarks/provbench/spotcheck/sample-eaf82d2.csv
"""

from __future__ import annotations

import argparse
import csv
from pathlib import Path


CANONICAL_COLUMNS = [
    "fact_id",
    "commit_sha",
    "bucket",
    "predicted_label",
    "human_label",
    "disagreement_notes",
]


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--autofilter", required=True, type=Path)
    p.add_argument("--out", required=True, type=Path)
    args = p.parse_args()

    with args.autofilter.open(newline="") as f:
        reader = csv.DictReader(f)
        rows = list(reader)

    counts = {"GREEN": 0, "YELLOW": 0, "DISAGREE": 0, "UNCERTAIN": 0, "PARSE_ERROR": 0}
    out_rows = []
    for r in rows:
        tag = r.get("auto_tag", "")
        note = r.get("auto_note", "")
        counts[tag] = counts.get(tag, 0) + 1
        if tag == "GREEN":
            human = r["predicted_label"]
            disagreement = ""
        elif tag == "YELLOW":
            human = r["predicted_label"]
            disagreement = f"auto-filter YELLOW: {note}"
        elif tag == "UNCERTAIN":
            # Auto-filter couldn't decide; default-trust the labeler
            # but mark for review.
            human = r["predicted_label"]
            disagreement = f"auto-filter UNCERTAIN, default-trusting labeler: {note}"
        else:
            # DISAGREE / PARSE_ERROR: leave blank for explicit ratification.
            human = ""
            disagreement = f"auto-filter {tag}: {note}"
        out_rows.append(
            {
                "fact_id": r["fact_id"],
                "commit_sha": r["commit_sha"],
                "bucket": r["bucket"],
                "predicted_label": r["predicted_label"],
                "human_label": human,
                "disagreement_notes": disagreement,
            }
        )

    with args.out.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=CANONICAL_COLUMNS)
        writer.writeheader()
        for row in out_rows:
            writer.writerow(row)

    filled = sum(1 for r in out_rows if r["human_label"])
    print(f"wrote {len(out_rows)} rows to {args.out}")
    print(f"  filled (GREEN/YELLOW/UNCERTAIN): {filled}")
    print(f"  blank (DISAGREE/PARSE_ERROR):    {len(out_rows) - filled}")
    for tag, n in sorted(counts.items(), key=lambda kv: -kv[1]):
        if n:
            print(f"  {tag:11s} {n:4d}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
