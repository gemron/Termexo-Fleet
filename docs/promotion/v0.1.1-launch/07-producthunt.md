# Product Hunt Launch Fields — Termexo Fleet v0.1.1

Submit URL: <https://www.producthunt.com/posts/new>

## Product Name
`Termexo Fleet`

## Tagline (<= 60 chars)
`Local-first AI coding fleet commander with zero black-box`

## Short Description (<= 260 chars)
`Orchestrate multiple coding agents (Claude Code, Codex CLI, OpenCode) across machines and accounts. Grade tasks L1–L4, force cross-family audits on L4, spawn worktree-isolated sub-agents via reverse MCP, and inspect or reply to every live PTY terminal.`

## Topics
`Developer Tools` · `Artificial Intelligence` · `Open Source` · `Productivity` · `Mac`

## Links
- Website: `https://www.termexo.com/#fleet`
- GitHub: `https://github.com/gemron/Termexo-Fleet`
- Release v0.1.1: `https://github.com/gemron/Termexo-Fleet/releases/tag/v0.1.1`
- npm: `https://www.npmjs.com/package/termexo-fleet`

## Media Assets to Upload
1. **Product Demo Video (16:9 1080p60, 45s)**: `D:\devlop\Termexo-Fleet\docs\promo\termexo-fleet-promo.mp4`
2. **Hero Cover Thumbnail**: `D:\devlop\Termexo-Fleet\docs\promo\termexo-fleet-promo-cover.png`
3. **Gallery Screenshots**: `docs/preview/screenshots/01-mission-deck.png` through `07-command-palette.png`

---

## First Maker Comment (Ready to Paste)

Hi Product Hunt! I'm the maintainer of **Termexo Fleet** (MIT open-source, built with Rust, Tauri 2, and Angular 22).

*(Disclosure: This launch note was drafted with AI assistance and verified directly against the v0.1.1 source and release notes. All screenshots and the 45-second video are captured from the real desktop build.)*

Running Claude Code, Codex CLI, and OpenCode across multiple accounts and machines (like a Windows workstation + a LAN Mac Studio) quickly surfaced four pain points for me:

1. **Burning flagship quota on trivial edits** (README fixes, comments, lint sweeps).
2. **Manual baby-sitting on `429` rate limits** or expired logins.
3. **Single-model blind spots** when the same model family writes and reviews critical concurrency or security changes.
4. **Black-box orchestrators** that hide terminal execution and make human approval or mid-flight steering painful.

**Termexo Fleet v0.1.1** sits on top of one or more [Termexo](https://github.com/gemron/Termexo) desktops as a transparent, local-first control plane:

- **Zero Black-Box Live Terminal Drawer**: Every task and sub-agent runs inside a real Termexo PTY terminal. Click any DAG node to watch live output or click `Approve (y)` right from the drawer.
- **L1–L4 Difficulty Ladder & Cross-Family L4 Audits**: Preview the exact node, account, and model before dispatching. `L1` routes routine edits to fast low-cost slots; `L4` automatically excludes the upstream author's model family (e.g., Claude implements → Codex independently reviews).
- **Reverse MCP Sub-Agent Swarms (`127.0.0.1:7425/mcp`)**: Parent agents can submit multi-stage DAGs (validated with Kahn's algorithm) and spawn child agents isolated in separate Git Worktrees under strict depth, concurrency, and budget guardrails.
- **Local SQLite Ledger & Self-Healing Failover**: Auto-cools down failing `429/402` slots, escalates retries, quantifies estimated savings vs. all-L3 usage locally, and dispatches Feishu / WeCom / Slack / WeChat alerts.

### Try It Locally
Windows x64, macOS (Apple Silicon & Intel), and Linux x64 builds are bundled into one npm package (Node.js 18.18+):

```bash
npx termexo-fleet@latest
```

**Current caveats**: Requires at least one running Termexo instance plus your own coding-agent CLIs/accounts. Windows installers are currently unsigned and macOS builds use ad-hoc signing (you may need to allow first launch in Privacy & Security).

I'd love feedback from anyone running multi-agent or multi-machine coding setups — especially on routing heuristics and guardrail defaults!

[GitHub](https://github.com/gemron/Termexo-Fleet) · [v0.1.1 Releases](https://github.com/gemron/Termexo-Fleet/releases/tag/v0.1.1) · [45s Tour](https://www.termexo.com/#fleet)
