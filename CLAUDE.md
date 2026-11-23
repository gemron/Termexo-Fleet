# CLAUDE.md — Termexo Fleet

Termexo Fleet is a local-first desktop AI fleet commander (Tauri 2 + Rust + Angular 22 + Tailwind CSS 4 + DaisyUI 5) that orchestrates one or more Termexo instances over Streamable HTTP MCP.

## Commands

```sh
npm --prefix apps/desktop-ui install    # Install Angular 22 dependencies
npm run dev                             # Browser preview on http://127.0.0.1:4201
npm run build                           # Angular production build -> apps/desktop-ui/dist/
npm test                                # Frontend Vitest unit tests
scripts\cargo-msvc.cmd test --workspace # Rust workspace tests (Windows MSVC)
npm run tauri:dev                       # Full Tauri 2 desktop application
```

## Architecture Notes

- **Zero black-box execution**: All tasks and child agents execute inside real Termexo terminals/task boards via `fleet-connector`.
- **Difficulty routing (`fleet-router`)**: Evaluates `L1`–`L4` difficulty, selects optimal `(node, agent, accountProfileId, modelProfileId, modelName)` slots, cools down rate-limited slots, and enforces cross-family diversity for `L4` reviews.
- **Autonomous sub-agents (`fleet-mcp-server` + `fleet-orchestrator`)**: Exposes local Streamable HTTP MCP on `http://127.0.0.1:7425/mcp` with Bearer auth, guardrail limits (`max_depth`, `max_concurrency`, `max_subagents`, duplicate detection), and optional isolated Git worktrees.
- **Secrets boundary**: Node MCP access tokens and Fleet's own MCP token are stored in the OS credential manager via `keyring`, never in SQLite.
