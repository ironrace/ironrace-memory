# IronRace Collab

`ironrace-memory` includes an experimental bounded planning protocol that lets
Claude Code and Codex coordinate through the same SQLite-backed MCP server.

This document explains:

- what the collab tools do
- how the bounded planning flow works
- how Claude and Codex know whose turn it is
- how to run a live manual session
- how the runtime, MCP server, and agent skill fit together

## What It Is

IronRace Collab v1 is a **bounded planning stage**, not a general autonomous
multi-agent framework.

It is designed for one specific workflow:

1. Claude and Codex each submit one independent first draft
2. Claude synthesizes both drafts into one canonical plan
3. Codex performs exactly one review pass
4. Claude publishes the final plan
5. The session terminates as `PlanLocked` or `PlanEscalated`

There is no open-ended planning loop in v1.

## Runtime Model

The stack is:

```text
Claude / Codex
  -> collab skill or command wrapper
  -> ironmem_collab_* MCP tools
  -> ironmem serve
  -> SQLite
```

Components:

- `ironmem` binary: protocol enforcement and persistence
- MCP server (`ironmem serve`): exposes the collab tools over stdio
- SQLite database: stores sessions, queued messages, capabilities, and WAL audit
- agent-side skill: tells Claude or Codex how to participate in the protocol

The protocol itself lives in the backend, not in the skill.

## Session State

Each collab session is stored in `collab_sessions`.

Important fields:

- `phase`: current protocol phase
- `current_owner`: the agent currently allowed to perform the next protocol step
- `claude_draft_hash`
- `codex_draft_hash`
- `canonical_plan_hash`
- `final_plan_hash`
- `codex_review_verdict`

Messages are stored in `messages`.
Capabilities are stored in `agent_capabilities`.

All writes are recorded in `wal_log`.

## Phase Model

### `PlanParallelDrafts`

Both agents may submit exactly one `draft`.

This is the only phase where turn-taking is relaxed. `current_owner` is not used
to block the two initial draft submissions.

Allowed actions:

- Claude sends one `draft`
- Codex sends one `draft`

Exit condition:

- once both draft hashes are present, phase becomes `PlanSynthesisPending`

### `PlanSynthesisPending`

Expected owner: `claude`

Allowed action:

- Claude sends one `canonical` message

Exit condition:

- phase becomes `PlanCodexReviewPending`

### `PlanCodexReviewPending`

Expected owner: `codex`

Allowed action:

- Codex sends one `review`

Allowed review verdicts:

- `approve`
- `approve_with_minor_edits`
- `request_changes`

Exit condition:

- phase becomes `PlanClaudeFinalizePending`

### `PlanClaudeFinalizePending`

Expected owner: `claude`

Allowed action:

- Claude sends one `final`

Exit condition:

- `PlanLocked` if Codex did not request changes
- `PlanEscalated` if Codex review verdict was `request_changes`

Important:

- Claude does **not** self-report whether Codex still objects
- lock vs escalate is derived from the stored Codex verdict

### `PlanLocked`

Terminal success state.

### `PlanEscalated`

Terminal non-converged state that requires human review.

## How Turn-Taking Works

Turn-taking is explicit.

Agents do not infer it from timing. They read it from session state.

To know whether it is their turn, an agent calls:

- `ironmem_collab_status(session_id)`

The response includes:

- `phase`
- `current_owner`

Protocol messages are rejected if the wrong agent acts in a strict-owner phase.

Special case:

- `PlanParallelDrafts` allows both agents to submit exactly one draft regardless
  of `current_owner`

In practice, each agent should:

1. call `ironmem_collab_status`
2. if needed, call `ironmem_collab_recv`
3. `ironmem_collab_ack` consumed messages
4. act only if the phase and owner allow it

## MCP Tools

### `ironmem_collab_start`

Creates a new session.

Inputs:

- `repo_path`
- `branch`
- `initiator`

Returns:

- `session_id`

### `ironmem_collab_send`

Sends a protocol message and advances the state machine when the topic is a
protocol topic.

Inputs:

- `session_id`
- `sender`
- `topic`
- `content`

Protocol topics:

- `draft`
- `canonical`
- `review`
- `final`

Unknown collab topics are rejected in v1.

Returns:

- `message_id`
- `phase`

### `ironmem_collab_recv`

Returns pending messages for the receiver.

Inputs:

- `session_id`
- `receiver`
- `limit` optional

### `ironmem_collab_ack`

Marks a message as consumed.

Inputs:

- `session_id`
- `message_id`

Important:

- ack is session-scoped
- a mismatched `(session_id, message_id)` returns an error

### `ironmem_collab_status`

Returns the full session state.

Use this before every protocol action.

### `ironmem_collab_approve`

Codex-only shortcut for an `approve` review.

Inputs:

- `session_id`
- `agent = "codex"`
- `content_hash`

Important:

- `content_hash` must match the stored `canonical_plan_hash`

### `ironmem_collab_register_caps`

Registers the current agent's capabilities for the session.

Capabilities are advisory only.

### `ironmem_collab_get_caps`

Returns capabilities for one agent or all registered agents in the session.

## Payload Formats

### Draft

Topic: `draft`

Content is free-form text.

Recommended structure:

```text
Goal
- ...

Constraints
- ...

Plan
1. ...
2. ...

Risks
- ...
```

### Canonical

Topic: `canonical`

Content is the merged canonical plan text.

### Review

Topic: `review`

Content must be JSON:

```json
{
  "verdict": "approve_with_minor_edits",
  "notes": ["...", "..."]
}
```

### Final

Topic: `final`

Content must be JSON:

```json
{
  "plan": "final merged plan text"
}
```

## Manual Live Session

### 1. Start the MCP server

Trusted mode is required for collab writes:

```bash
IRONMEM_MCP_MODE=trusted ./target/release/ironmem serve
```

For smoke tests without the model:

```bash
IRONMEM_MCP_MODE=trusted IRONMEM_EMBED_MODE=noop ./target/release/ironmem serve
```

### 2. Create a session

Have one side call `ironmem_collab_start`.

Example intent:

- repo path = current checkout
- branch = active branch
- initiator = `claude`

### 3. Register capabilities

Both agents should call `ironmem_collab_register_caps`.

### 4. Submit independent drafts

- Claude submits one `draft`
- Codex submits one `draft`

### 5. Claude synthesizes

When `phase = PlanSynthesisPending`, Claude sends `canonical`.

### 6. Codex reviews

When `phase = PlanCodexReviewPending`, Codex sends `review` or uses
`ironmem_collab_approve`.

### 7. Claude finalizes

When `phase = PlanClaudeFinalizePending`, Claude sends `final`.

### 8. Read the terminal state

Call `ironmem_collab_status`.

Expected final state:

- `PlanLocked`, or
- `PlanEscalated`

## Suggested Operator Prompts

Use an agent-side skill or command wrapper to normalize operator behavior.

Recommended command forms:

- `/collab start <task>`
- `/collab join <session_id> <task>`
- `/collab continue <session_id>`

The wrapper should:

- start or join the session
- register capabilities
- obey strict phase/owner checks
- never create extra draft or review rounds

## Validation Commands

Backend validation:

```bash
cargo test -p ironrace-memory collab::
cargo test -p ironrace-memory --test mcp_protocol
cargo test -p ironrace-memory
cargo clippy -p ironrace-memory -- -D warnings
```

Release + tool surface smoke test:

```bash
cargo build -p ironrace-memory --release
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' \
  | env HOME=/tmp/ironmem-home IRONMEM_EMBED_MODE=noop IRONMEM_MCP_MODE=trusted ./target/release/ironmem serve --db /tmp/ironmem-collab-tools.sqlite3 \
  | python3 -c "import sys,json; t=[x['name'] for x in json.load(sys.stdin)['result']['tools']]; assert all(f'ironmem_collab_{n}' in t for n in ['start','send','recv','ack','status','approve','register_caps','get_caps']), t; print('OK')"
```

## Current Scope and Known Limits

Current scope:

- bounded planning only
- no implementation loop
- no PR creation / review integration

Known limits:

- agents must still be explicitly pointed at a `session_id`
- this is not yet an autonomous dispatcher
- the current docs assume a skill or command wrapper exists on the client side

## Recommended Packaging

Treat the system as four layers:

1. `ironmem` binary
2. `ironmem serve` MCP server
3. SQLite shared database
4. `collab` skill / command shim

That keeps protocol enforcement in the backend and agent-specific workflow in the
client layer.
