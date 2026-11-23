# Termexo Fleet Architecture

## 1. Control Plane vs Execution Plane

- **Termexo (`Worker Node`)**: Runs real PTY terminals, isolates accounts/credentials, manages model profiles, workspaces, and task boards, and exposes a local Streamable HTTP MCP server.
- **Termexo Fleet (`Control Plane`)**: Connects to one or more Termexo nodes, routes tasks by difficulty (`L1`–`L4`), manages account health/cooldowns, enforces sub-agent guardrails and Git worktree isolation, and exposes a reverse MCP server so parent agents running inside Termexo can spawn child agents safely.

## 2. Crate Layout

| Crate | Responsibility |
| --- | --- |
| `crates/fleet-protocol` | Shared domain models (`FleetNode`, `AccountModelSlot`, `DifficultyTier`, `Mission`, `MissionTask`, `GuardrailPolicy`, `AttentionItem`). |
| `crates/fleet-connector` | Streamable HTTP MCP client for Termexo + terminal output signal classifier (`rate_limited`, `auth_expired`, `waiting_approval`). |
| `crates/fleet-router` | Automatic difficulty evaluator, `cost`/`balanced`/`quality` slot scoring, `L4` cross-family model diversity, cooldown failover, and retry escalation. |
| `crates/fleet-orchestrator` | Sub-agent guardrail checks (`max_depth`, `max_concurrency`, `max_subagents`, duplicate detection), prompt context injection, and Git worktree lifecycle. |
| `crates/fleet-mcp-server` | Local Axum Streamable HTTP MCP server (`127.0.0.1:7425/mcp`) exposing `fleet_spawn_subagent`, `fleet_list_subagents`, `fleet_wait_subagents`, `fleet_message_subagent`, and `fleet_cancel_subagent`. |
| `src-tauri` | Tauri 2 desktop shell, SQLite (`rusqlite`) state, OS `keyring` token storage, periodic node sync, and IPC commands for Angular 22. |

## 3. Reverse Sub-Agent MCP Flow

1. In Termexo, create an **MCP Profile** named `Termexo Fleet` using the JSON snippet shown in the **Fleet Nodes** tab.
2. When launching a Mission in Termexo Fleet, select that MCP Profile. Fleet passes `mcpProfileIds` on `task_create` (supported in Termexo for Claude Code, Codex CLI, and OpenCode) and injects a signed `parent_token` into the parent task prompt.
3. During execution, the parent agent calls `fleet_spawn_subagent(parent_token, title, goal, difficulty, isolate_worktree)`.
4. Fleet verifies guardrails, selects the best node/account/model slot, creates an isolated Git worktree (`fleet/<subagent-id>`), creates and launches the child task on Termexo, and returns structured summaries and `git diff --stat` when `fleet_wait_subagents` completes.
