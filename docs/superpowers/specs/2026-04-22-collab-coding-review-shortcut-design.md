# Collab Coding-Review Shortcut

**Date:** 2026-04-22
**Status:** Design — pending implementation plan
**Scope:** Add one MCP tool + one slash command that drops an IronRace Collab session directly into the v3 global-review stage. No changes to existing v1 planning, v3 per-task coding, or Codex's `review_fix` write authority.

## Motivation

`superpowers:subagent-driven-development` already orchestrates a full per-task coding loop on a single branch: implementer subagent commits → spec-reviewer subagent → code-quality-reviewer subagent → re-dispatch on issues → next task. When that loop finishes, the branch is ready for a second-model review before PR.

Today, getting Codex to do that review via Collab requires either running v1 planning (redundant — the plan already executed) or running v3 per-task coding from scratch (redundant — the coding already happened). Both paths re-litigate work already done.

This design adds a narrow shortcut: one call starts a session positioned exactly where v3's existing global-review stage lives.

## Non-Goals

- No changes to v3 state machine semantics, phase names, events, or Codex's ability to push fixes directly during `CodeReviewFixGlobal`.
- No changes to v1 planning.
- No per-task collab coordination for orchestrators taking this path — all per-task work is assumed complete before the shortcut is called.
- No support for looping back into per-task phases from a shortcut session. Shortcut sessions are global-review-only and terminate at `CodingComplete` / `CodingFailed`.

## Design

### New MCP tool: `collab_start_code_review`

Creates a new session whose initial phase is `CodeReviewFixGlobalPending` with `current_owner = codex`.

**Input:**

```json
{
  "repo_path": "/path/to/repo",
  "branch": "feat/xyz",
  "base_sha": "abc123...",
  "head_sha": "def456...",
  "initiator": "claude",
  "task": "short human description of what was built"
}
```

- `base_sha`: commit before subagent-driven-dev started (used for drift detection).
- `head_sha`: current branch HEAD after all subagent work completed.
- `initiator`: must be `claude`. Codex cannot initiate a shortcut session (Codex's role in this flow is reviewer, not driver).
- `task`: stored on the session for `collab_status` visibility; typically the top-level goal from the plan that was just executed.

**Output:** `{ session_id, task }`.

**Session state at creation:**

- `phase = CodeReviewFixGlobalPending`
- `current_owner = codex`
- `base_sha = <input>`
- `last_head_sha = <input.head_sha>`
- `task = <input>`
- `task_list = null`, `current_task_index = null`
- `plan_hash = null`, `final_plan_hash = null`, `canonical_plan_hash = null`
- No planning-phase fields populated

**The handshake turn is collapsed.** The existing global-review stage is three turns: Claude `ReviewLocal` → Codex `CodeReviewFixGlobal` → Claude `FinalReview`. The first turn is a no-op handshake — its only payload is `head_sha`, which the shortcut already receives at session creation. Starting in `CodeReviewFixGlobalPending` saves a round-trip with no loss of information.

### Flow from the shortcut

From session creation onward, the existing v3 global-review transitions run unchanged:

| Phase | Owner | Event | Next |
|---|---|---|---|
| `CodeReviewFixGlobalPending` | `codex` | `CodeReviewFixGlobal{head_sha}` — Codex reviewed the full branch and (if needed) pushed fixes directly | `CodeReviewFinalPending` |
| `CodeReviewFinalPending` | `claude` | `FinalReview{head_sha, pr_url}` — Claude opens the PR and sends the URL in the same event | `CodingComplete` (terminal) |

Failure path also unchanged: `FailureReport{coding_failure}` from either agent during either phase transitions to `CodingFailed`.

### Slash command: `/collab review`

Extend the existing `/collab` command with a new subcommand. The user types a short topic; the agent auto-fills everything else by inspecting the local repo.

**Usage:**

```
/collab review <short-topic>
```

**Agent responsibilities:**

1. Resolve `repo_path` to the current working directory's repo root.
2. Resolve `branch` to the current checked-out branch (reject if detached HEAD or on `main`/`master`).
3. Resolve `head_sha` to current HEAD.
4. Resolve `base_sha` to `git merge-base origin/main HEAD` (or the default branch's merge-base).
5. Call `collab_start_code_review` with those values + the user's short topic as `task`.
6. Report `session_id` and the `collab-join <sid>` string back to the user so Codex can join.

This matches the existing "user types only the task" ergonomic documented in the project's memory.

## Invariants and Edge Cases

- **Drift detection.** The shortcut reuses the existing `base_sha` / `last_head_sha` machinery. When Codex submits `CodeReviewFixGlobal{head_sha}`, the server verifies the new HEAD is a descendant of `last_head_sha` (or equal, if Codex pushed no fixes). No new drift logic is needed.
- **Terminal set.** For shortcut sessions, `wait_my_turn` treats `{CodingComplete, CodingFailed}` as terminal from session creation. There is no `PlanLocked` window to account for.
- **`collab_end` rejection.** `collab_end` is rejected during both `CodeReviewFixGlobalPending` and `CodeReviewFinalPending` — same rule that applies to any coding-active phase today. The only escape from a stuck shortcut session is `failure_report`.
- **Capabilities registration.** Codex must still register capabilities via `collab_register_caps` before submitting its review, same as in the existing global-review stage. The shortcut does not change the capability contract.
- **Re-entry.** A shortcut session is single-use. Once it reaches `CodingComplete` or `CodingFailed`, the orchestrator creates a new session for any subsequent review cycle.
- **Initiator restriction.** `initiator = codex` is rejected at the tool boundary. This prevents Codex from unilaterally starting a review session on work it hasn't seen.

## What Changes in the Codebase

This section is scoped to orient an implementation plan, not to prescribe edits.

- `crates/ironmem/src/collab/session.rs` and `state_machine/mod.rs` — add a session-creation path that seeds `phase = CodeReviewFixGlobalPending`, `current_owner = codex`, `base_sha`, `last_head_sha`, `task`; leave all planning-phase and per-task fields null.
- `crates/ironmem/src/mcp/tools/collab_session.rs` — add the `collab_start_code_review` MCP tool registration, input validation, and session-creation call.
- `crates/ironmem/src/collab/state_machine/tests.rs` — tests covering: shortcut session creation with valid inputs; initiator=codex rejected; drift detection from `base_sha` through `CodeReviewFixGlobal`; happy path through `FinalReview` → `CodingComplete`; `failure_report` through `CodingFailed`; `collab_end` rejection during both phases.
- `crates/ironmem/src/db/schema.rs` — no new columns expected; existing `collab_sessions` fields cover the shortcut state.
- `docs/COLLAB.md` — add a "Shortcut: post-subagent coding review" subsection near the v3 coding section, documenting the tool, the skipped handshake, and the surviving global-review flow.
- `.claude-plugin/commands/collab.md` and `.codex-plugin/prompts/collab.md` — add `/collab review <short-topic>` with the auto-fill rules above.

## Compatibility

Non-breaking. No existing tool signatures change. No existing phase or event names change. No existing session records are affected. Orchestrators that want the full v1 + v3 flow call `collab_start` as before; orchestrators that want post-subagent review call `collab_start_code_review`.

## Deferred to Implementation Plan

No open design questions. Implementation details to resolve during planning: exact error codes for invalid SHAs, behavior when `base_sha == head_sha` (empty-branch sessions), and how `/collab review` detects the default branch in repos that use `master` instead of `main`.
