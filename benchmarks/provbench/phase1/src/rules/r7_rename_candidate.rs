//! R7 rename_candidate — SPEC §5.2.
//!
//! Fires when the bound source file is **absent** at the commit but a
//! same-extension path in the commit tree has a basename similar to the
//! fact's leaf symbol — interprets the disappearance as a rename rather
//! than a deletion. Returns `Stale` (reason `stale_symbol_renamed`).
//!
//! Deterministic tie-break: `(similarity desc, qualified_name asc)`.
//!
//! Reachability: this rule is placed **ahead of R1** in `RuleChain::default()`
//! so it gets first crack at the `post_blob.is_none()` rows. Without that
//! ordering R1 (`stale_source_deleted`) would consume every deleted-file row
//! before R7 could distinguish a rename. The `post_blob.is_some()` guard is
//! kept: when the file still exists, R7 has no signal to work with (it
//! would need to read blob content at other paths, which `RowCtx` does not
//! carry); fall through to R2 / R5 / R6 / R3 / R4 instead.
//!
//! Heuristic note: the previous proxy compared `fact.body` (multi-token
//! English prose) to `path` (a slash-separated filesystem path). Their
//! Jaccard overlap was near zero, so R7 essentially never fired even on
//! the rows it was supposed to handle. The new proxy compares the
//! qualified-symbol leaf segment (e.g. `Walk::new` -> `"new"`) to each
//! candidate path's file stem and only considers candidates that share
//! the original file's extension — a structural heuristic that does not
//! require reading the candidate blob's contents.

use super::{Decision, RowCtx, Rule};
use crate::similarity::{similarity, RENAME_THRESHOLD};
use std::path::Path;

pub struct R7RenameCandidate;

impl Rule for R7RenameCandidate {
    fn rule_id(&self) -> &'static str {
        "R7"
    }
    fn spec_ref(&self) -> &'static str {
        "SPEC §5.2"
    }
    fn classify(&self, ctx: &RowCtx<'_>) -> Option<(Decision, String)> {
        // Only applies to symbol-bearing fact kinds.
        if !matches!(
            ctx.fact.kind.as_str(),
            "FunctionSignature" | "Field" | "PublicSymbol"
        ) {
            return None;
        }
        // The file still exists at the commit; R7 cannot distinguish a
        // rename from any other kind of change without reading blobs at
        // other paths. Defer to R2 / R5 / R6 / R3 / R4.
        if ctx.post_blob.is_some() {
            return None;
        }

        // Leaf segment of the qualified symbol (e.g. `Walk::new -> "new"`).
        // Falls back to the whole `symbol_path` if there is no `::`.
        let leaf = ctx
            .fact
            .symbol_path
            .rsplit("::")
            .next()
            .unwrap_or(&ctx.fact.symbol_path);
        if leaf.is_empty() {
            return None;
        }

        // Same extension as the original source path (renames almost always
        // preserve language). `None` extension matches `None` extension.
        let orig_ext = Path::new(&ctx.fact.source_path)
            .extension()
            .and_then(|e| e.to_str());

        let mut best: Option<(f32, &str)> = None;
        for path in ctx.commit_files {
            if path == &ctx.fact.source_path {
                continue;
            }
            let p = Path::new(path.as_str());
            if p.extension().and_then(|e| e.to_str()) != orig_ext {
                continue;
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let s = similarity(leaf, stem);
            if s >= RENAME_THRESHOLD {
                best = match best {
                    Some((bs, bp)) if bs > s || (bs == s && bp <= path.as_str()) => Some((bs, bp)),
                    _ => Some((s, path.as_str())),
                };
            }
        }
        if let Some((s, p)) = best {
            // Preserve 3-decimal similarity by parsing the formatted
            // string back into a JSON number; the round-trip avoids
            // f32 precision noise in the trace artifact while keeping
            // `to` safely escaped via serde_json.
            let similarity_value: serde_json::Value = format!("{s:.3}")
                .parse::<f64>()
                .map(serde_json::Value::from)
                .unwrap_or(serde_json::Value::Null);
            return Some((
                Decision::Stale,
                serde_json::json!({
                    "rule": "R7",
                    "reason": "stale_symbol_renamed",
                    "similarity": similarity_value,
                    "leaf": leaf,
                    "to": p,
                })
                .to_string(),
            ));
        }
        None
    }
}
