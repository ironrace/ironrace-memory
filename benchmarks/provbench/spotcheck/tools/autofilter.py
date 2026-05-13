#!/usr/bin/env python3
"""Independent auto-filter for the ProvBench spot-check CSV.

Goal — implement a minimal, labeler-independent re-derivation of each
row's expected label using only `git cat-file` and regex against the
pilot ripgrep clone. Triage tags:

  GREEN     auto-derived label matches predicted_label; the row can be
            fast-tracked (human_label = predicted_label).
  YELLOW    auto-derived label is ambiguous or partially matches;
            surfaced to the human reviewer with the heuristic note.
  DISAGREE  auto-derived label clearly differs from predicted_label;
            surfaced to the human reviewer with both labels + note.
  UNCERTAIN the auto-filter could not decide (e.g., regex didn't bite,
            file binary, etc.); surfaced to the human reviewer.

This script intentionally re-implements the fact-checks from scratch
rather than reusing labeler code, so it can serve as an independent
control per SPEC §9.1. False matches on "GREEN" rows would dilute the
agreement metric, so the green-path checks are deliberately strict;
when in doubt, the row escalates to a human.

Usage::

    python3 benchmarks/provbench/spotcheck/tools/autofilter.py \
        --csv benchmarks/provbench/spotcheck/sample-e96c9fe.csv \
        --repo benchmarks/provbench/work/ripgrep \
        --t0 af6b6c543b224d348a8876f0c06245d9ea7929c5 \
        --out benchmarks/provbench/spotcheck/sample-e96c9fe-autofilter.csv

Output is the input CSV widened by two columns: `auto_tag` and
`auto_note`. The `provbench-labeler report` tool reads only the
canonical 6 columns, so the widened CSV is reference-only.
"""

from __future__ import annotations

import argparse
import csv
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

T0_DEFAULT = "af6b6c543b224d348a8876f0c06245d9ea7929c5"

# ---------------------------------------------------------------------
# git plumbing
# ---------------------------------------------------------------------


def git_cat_file(repo: Path, sha: str, path: str) -> Optional[bytes]:
    """Return the bytes of `<sha>:<path>` or None if the path doesn't
    exist in that tree. Never raises on missing-blob — that's a normal
    signal we use to detect deletions."""
    try:
        result = subprocess.run(
            ["git", "-C", str(repo), "cat-file", "-p", f"{sha}:{path}"],
            capture_output=True,
            check=False,
        )
    except FileNotFoundError as exc:
        sys.exit(f"git not on PATH: {exc}")
    if result.returncode != 0:
        # Most common cause: blob doesn't exist at that path in that
        # tree. Less common: invalid sha. We treat both as 'absent'.
        return None
    return result.stdout


def git_ls_tree_paths(repo: Path, sha: str) -> set[str]:
    """All file paths in the tree at `sha` (recursive)."""
    result = subprocess.run(
        ["git", "-C", str(repo), "ls-tree", "-r", "--name-only", sha],
        capture_output=True,
        check=True,
        text=True,
    )
    return set(result.stdout.splitlines())


# ---------------------------------------------------------------------
# fact-id parsing
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class FactId:
    kind: str
    # name carries the leaf for PublicSymbol/FunctionSignature/TestAssertion/DocClaim;
    # for Field it carries "<container>::<field>".
    name: str
    path: str
    line: int

    @property
    def leaf(self) -> str:
        """Return the rightmost `::`-segment of `name` — for a
        namespaced function (`tests::buffer_zero_capacity`) this is
        the bare function name (`buffer_zero_capacity`) that actually
        appears after `fn ` in source. For Field this is the field
        name (e.g. `any_uppercase`)."""
        return self.name.rsplit("::", 1)[-1]

    @property
    def container(self) -> Optional[str]:
        """For Field, return the struct/enum container (everything
        before the final `::` in `name`); else `None`."""
        if self.kind == "Field":
            head, _, _ = self.name.rpartition("::")
            return head or None
        return None


def parse_fact_id(fact_id: str) -> Optional[FactId]:
    """Parse a fact_id of the form `<Kind>::<name>::<path>::<line>`,
    where `<name>` may itself contain `::` (e.g. namespaced test
    functions like `tests::buffer_binary_convert2`).

    The trailing line is always an integer. The path is always the
    last `::`-segment containing a `/` (since fact_ids encode
    forward-slash-normalized relative paths). For Field facts the
    name carries an explicit container::field pair which we keep
    joined as a single string.
    """
    parts = fact_id.split("::")
    if len(parts) < 4 or parts[-1] == "":
        return None
    try:
        line = int(parts[-1])
    except ValueError:
        return None
    # Locate the path segment by walking from the end: the right-most
    # non-line segment that contains `/` is the path. If no segment
    # contains `/`, fall back to the segment immediately preceding
    # the line (some top-level files like `README.md` have no `/`).
    path_idx: Optional[int] = None
    for i in range(len(parts) - 2, 0, -1):
        if "/" in parts[i]:
            path_idx = i
            break
    if path_idx is None:
        path_idx = len(parts) - 2
    path = parts[path_idx]
    kind = parts[0]
    if kind == "Field":
        # Field::<container>::<field>::<path>::<line>; container and
        # field together live in parts[1..path_idx], which for the
        # canonical case is exactly two segments.
        name_segments = parts[1:path_idx]
        if len(name_segments) < 2:
            return None
        name = "::".join(name_segments)
        return FactId(kind=kind, name=name, path=path, line=line)
    # All other kinds: name = everything between kind and path.
    name = "::".join(parts[1:path_idx])
    if not name:
        return None
    return FactId(kind=kind, name=name, path=path, line=line)


# ---------------------------------------------------------------------
# regex helpers — kept loose; we want presence, not perfect parsing.
# ---------------------------------------------------------------------


def _word(s: str) -> str:
    return re.escape(s)


def has_bare_pub_symbol(text: str, name: str) -> bool:
    """True if the file contains `pub <something> <name>` at any kind.
    `<something>` is fn/struct/enum/trait/type/const/static/mod/union.

    Additionally accepts `pub use` re-exports where `<name>` appears as
    a leaf of the use-path: `pub use path::to::<name>;`,
    `pub use path::{<name>};`, or `pub use path::<name> as alias;`.
    Re-exports are part of the public surface even though the symbol
    itself lives elsewhere, so they keep `Fact::PublicSymbol` valid.
    Excludes `pub(crate)`, `pub(super)`, `pub(in path)`, and bare-private."""
    direct = re.compile(
        rf"\bpub\s+(?:fn|struct|enum|trait|type|const|static|mod|union)\b[^{{;\n]*\b{_word(name)}\b",
        re.MULTILINE,
    )
    # Bare `pub use` lines mentioning the name as a path leaf. We
    # restrict the match to a single logical statement (terminated by
    # `;`) so we don't bleed into adjacent items.
    pub_use = re.compile(
        rf"\bpub\s+use\b[^;]*\b{_word(name)}\b[^;]*;",
        re.MULTILINE | re.DOTALL,
    )
    return direct.search(text) is not None or pub_use.search(text) is not None


def has_narrowed_pub_symbol(text: str, name: str) -> bool:
    """True if the file contains `pub(crate|super|in ...) <kind> <name>`
    OR a non-pub `fn|struct|...` `<name>`. Narrowed visibility OR fully
    private both qualify as "no longer bare-public"."""
    narrowed = re.compile(
        rf"\bpub\s*\(\s*(?:crate|super|in\s+[^)]+)\s*\)\s+(?:fn|struct|enum|trait|type|const|static|mod|union|use)\b[^{{;\n]*\b{_word(name)}\b",
        re.MULTILINE,
    )
    private = re.compile(
        rf"^\s*(?:fn|struct|enum|trait|type|const|static|mod|union)\b[^{{;\n]*\b{_word(name)}\b",
        re.MULTILINE,
    )
    return narrowed.search(text) is not None or private.search(text) is not None


def has_fn_with_name(text: str, name: str) -> bool:
    """Function (any visibility) with the given name."""
    pat = re.compile(rf"\bfn\s+{_word(name)}\b", re.MULTILINE)
    return pat.search(text) is not None


def fn_signature_line(text: str, name: str) -> Optional[str]:
    """Return the line containing the `fn <name>(...)` signature start,
    stripped of leading whitespace. None if no such line."""
    pat = re.compile(rf"\bfn\s+{_word(name)}\b\s*[<(]", re.MULTILINE)
    for line in text.splitlines():
        if pat.search(line):
            return line.strip()
    return None


def all_fn_signature_lines(text: str, name: str) -> list[str]:
    """All lines containing `fn <name>(...)`, deduplicated, stripped.
    Used when a file may have multiple impl blocks defining the same
    function name."""
    pat = re.compile(rf"\bfn\s+{_word(name)}\b\s*[<(]", re.MULTILINE)
    seen: list[str] = []
    for line in text.splitlines():
        if pat.search(line):
            stripped = line.strip()
            if stripped not in seen:
                seen.append(stripped)
    return seen


def has_field_in_struct(text: str, container: str, field: str) -> bool:
    """Best-effort check: container struct/enum block contains a line
    starting with `<field>:`. We isolate the container body by brace
    counting from `struct <container>` or `enum <container>`."""
    decl = re.search(
        rf"\b(?:struct|enum)\s+{_word(container)}\b[^{{]*\{{", text, re.MULTILINE
    )
    if not decl:
        return False
    start = decl.end()
    depth = 1
    body_end = start
    for i, ch in enumerate(text[start:], start=start):
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                body_end = i
                break
    body = text[start:body_end]
    pat = re.compile(rf"^\s*{_word(field)}\s*:", re.MULTILINE)
    return pat.search(body) is not None


def has_inline_code_mention(text: str, qualified_name: str) -> bool:
    """Find a backtick-delimited inline-code span containing the
    qualified name as a whole word."""
    pat = re.compile(rf"`[^`\n]*\b{_word(qualified_name)}\b[^`\n]*`")
    return pat.search(text) is not None


# ---------------------------------------------------------------------
# kind-specific independent classifiers
# ---------------------------------------------------------------------


VALID = "valid"
STALE_CHANGED = "stale_source_changed"
STALE_DELETED = "stale_source_deleted"
RENAMED = "stale_symbol_renamed"
NEEDS_REVAL = "needs_revalidation"


@dataclass
class AutoVerdict:
    label: str  # one of {VALID, STALE_CHANGED, STALE_DELETED, RENAMED, NEEDS_REVAL, "uncertain"}
    note: str
    confidence: str  # "high" | "medium" | "low"


def classify_public_symbol(
    repo: Path, t0: str, sha: str, fid: FactId
) -> AutoVerdict:
    t0_bytes = git_cat_file(repo, t0, fid.path)
    post_bytes = git_cat_file(repo, sha, fid.path)
    if post_bytes is None:
        return AutoVerdict(STALE_DELETED, f"file `{fid.path}` absent at commit", "high")
    if t0_bytes is None:
        return AutoVerdict("uncertain", f"file `{fid.path}` absent at T₀; cannot verify symbol existed", "low")
    try:
        post_text = post_bytes.decode("utf-8", errors="replace")
    except Exception as exc:
        return AutoVerdict("uncertain", f"decode error: {exc}", "low")
    if has_bare_pub_symbol(post_text, fid.leaf):
        return AutoVerdict(VALID, f"`pub <kind> {fid.leaf}` still present", "high")
    if has_narrowed_pub_symbol(post_text, fid.leaf):
        return AutoVerdict(
            STALE_CHANGED,
            f"`{fid.leaf}` present but visibility narrowed or made private",
            "high",
        )
    return AutoVerdict(
        STALE_DELETED,
        f"no `pub … {fid.leaf}` and no narrowed equivalent in {fid.path}",
        "medium",
    )


def classify_function_signature(
    repo: Path, t0: str, sha: str, fid: FactId
) -> AutoVerdict:
    t0_bytes = git_cat_file(repo, t0, fid.path)
    post_bytes = git_cat_file(repo, sha, fid.path)
    if post_bytes is None:
        return AutoVerdict(STALE_DELETED, f"file `{fid.path}` absent at commit", "high")
    if t0_bytes is None:
        return AutoVerdict("uncertain", f"file `{fid.path}` absent at T₀", "low")
    t0_text = t0_bytes.decode("utf-8", errors="replace")
    post_text = post_bytes.decode("utf-8", errors="replace")
    t0_sigs = all_fn_signature_lines(t0_text, fid.leaf)
    post_sigs = all_fn_signature_lines(post_text, fid.leaf)
    if not post_sigs:
        if has_fn_with_name(post_text, fid.leaf):
            return AutoVerdict(
                "uncertain",
                f"fn `{fid.leaf}` present but signature regex didn't bite",
                "low",
            )
        return AutoVerdict(
            STALE_DELETED, f"fn `{fid.leaf}` absent from {fid.path}", "medium"
        )
    if not t0_sigs:
        return AutoVerdict(
            "uncertain", f"fn `{fid.leaf}` absent at T₀; cannot compare", "low"
        )
    # Same-named function may appear in multiple impl blocks. We can't
    # pinpoint THE fact's fn from the line number alone (the fact at a
    # post-commit isn't necessarily at the same line), so we only call
    # this VALID if BOTH sides have the exact same set of signature
    # lines. Any divergence — added/removed/modified — is reported as
    # STALE_CHANGED with medium confidence so it escalates to human
    # review for adjudication.
    t0_set = sorted(t0_sigs)
    post_set = sorted(post_sigs)
    if t0_set == post_set:
        confidence = "high" if len(t0_set) == 1 else "medium"
        return AutoVerdict(
            VALID,
            f"signature byte-identical ({len(t0_set)} occurrence(s))",
            confidence,
        )
    return AutoVerdict(
        STALE_CHANGED,
        f"signature lines differ: T₀={t0_set} post={post_set}",
        "medium",
    )


def classify_field(repo: Path, t0: str, sha: str, fid: FactId) -> AutoVerdict:
    container = fid.container or ""
    field = fid.leaf
    post_bytes = git_cat_file(repo, sha, fid.path)
    if post_bytes is None:
        return AutoVerdict(STALE_DELETED, f"file `{fid.path}` absent at commit", "high")
    post_text = post_bytes.decode("utf-8", errors="replace")
    if has_field_in_struct(post_text, container, field):
        return AutoVerdict(
            VALID, f"`{container}.{field}` still present in {fid.path}", "high"
        )
    return AutoVerdict(
        STALE_DELETED,
        f"`{container}.{field}` not found in {fid.path} (could be rename or true delete)",
        "medium",
    )


def classify_test_assertion(
    repo: Path, t0: str, sha: str, fid: FactId
) -> AutoVerdict:
    t0_bytes = git_cat_file(repo, t0, fid.path)
    post_bytes = git_cat_file(repo, sha, fid.path)
    if post_bytes is None:
        return AutoVerdict(STALE_DELETED, f"file `{fid.path}` absent at commit", "high")
    post_text = post_bytes.decode("utf-8", errors="replace")
    if not has_fn_with_name(post_text, fid.leaf):
        return AutoVerdict(
            STALE_DELETED, f"test fn `{fid.leaf}` absent from {fid.path}", "medium"
        )
    # Test fn is present but TestAssertion facts also pin the
    # assertion body content. The auto-filter does not parse fn
    # bodies, so we can only say "presence is consistent with valid"
    # — full ratification requires looking at the body. Compare the
    # whole-file byte-equality as a cheap proxy: if the file is
    # unchanged, the test body is unchanged.
    if t0_bytes is not None and t0_bytes == post_bytes:
        return AutoVerdict(
            VALID,
            f"test fn `{fid.leaf}` present and file `{fid.path}` byte-identical to T₀",
            "high",
        )
    return AutoVerdict(
        VALID,
        f"test fn `{fid.leaf}` still present in {fid.path}; body unchecked",
        "low",
    )


def classify_doc_claim(repo: Path, t0: str, sha: str, fid: FactId) -> AutoVerdict:
    post_bytes = git_cat_file(repo, sha, fid.path)
    if post_bytes is None:
        return AutoVerdict(STALE_DELETED, f"file `{fid.path}` absent at commit", "high")
    post_text = post_bytes.decode("utf-8", errors="replace")
    if has_inline_code_mention(post_text, fid.leaf):
        return AutoVerdict(
            VALID, f"inline `{fid.leaf}` mention still present in {fid.path}", "high"
        )
    return AutoVerdict(
        STALE_DELETED,
        f"no inline-code mention of `{fid.leaf}` in {fid.path}",
        "medium",
    )


CLASSIFIERS = {
    "PublicSymbol": classify_public_symbol,
    "FunctionSignature": classify_function_signature,
    "Field": classify_field,
    "TestAssertion": classify_test_assertion,
    "DocClaim": classify_doc_claim,
}


# ---------------------------------------------------------------------
# triage
# ---------------------------------------------------------------------


def triage(predicted: str, verdict: AutoVerdict) -> tuple[str, str]:
    """Return (auto_tag, auto_note)."""
    note = f"{verdict.label} ({verdict.confidence}): {verdict.note}"
    if verdict.label == "uncertain":
        return "UNCERTAIN", note
    if verdict.label == predicted:
        if verdict.confidence == "high":
            return "GREEN", note
        return "YELLOW", note
    # auto-derived label is unable to distinguish renamed/needs_reval vs
    # deleted with high confidence — escalate rather than disagree
    # outright in those cases.
    if predicted in {RENAMED, NEEDS_REVAL} and verdict.label == STALE_DELETED:
        return "YELLOW", (
            f"auto says deleted; labeler says {predicted}; "
            f"rename/reval check exceeds auto-filter scope: {verdict.note}"
        )
    return "DISAGREE", note


# ---------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--csv", required=True, type=Path)
    p.add_argument("--repo", required=True, type=Path)
    p.add_argument("--t0", default=T0_DEFAULT)
    p.add_argument("--out", required=True, type=Path)
    args = p.parse_args()

    with args.csv.open(newline="") as f:
        reader = csv.DictReader(f)
        rows = list(reader)
        fieldnames = list(reader.fieldnames or [])

    out_fieldnames = fieldnames + ["auto_tag", "auto_note"]
    counts = {"GREEN": 0, "YELLOW": 0, "DISAGREE": 0, "UNCERTAIN": 0, "PARSE_ERROR": 0}

    enriched = []
    for row in rows:
        fid = parse_fact_id(row["fact_id"])
        if fid is None:
            row["auto_tag"] = "PARSE_ERROR"
            row["auto_note"] = "could not parse fact_id"
            counts["PARSE_ERROR"] += 1
            enriched.append(row)
            continue
        classifier = CLASSIFIERS.get(fid.kind)
        if classifier is None:
            row["auto_tag"] = "PARSE_ERROR"
            row["auto_note"] = f"unknown fact kind `{fid.kind}`"
            counts["PARSE_ERROR"] += 1
            enriched.append(row)
            continue
        verdict = classifier(args.repo, args.t0, row["commit_sha"], fid)
        tag, note = triage(row["predicted_label"], verdict)
        row["auto_tag"] = tag
        row["auto_note"] = note
        counts[tag] = counts.get(tag, 0) + 1
        enriched.append(row)

    with args.out.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=out_fieldnames)
        writer.writeheader()
        for row in enriched:
            writer.writerow(row)

    total = sum(counts.values())
    print(f"wrote {total} rows to {args.out}")
    for tag, n in sorted(counts.items(), key=lambda kv: -kv[1]):
        if n:
            print(f"  {tag:11s} {n:4d}  ({n / total:5.1%})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
