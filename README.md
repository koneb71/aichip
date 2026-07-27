# aichip

**A local-first multi-agent workflow platform for coding agents — no API keys.**

aichip orchestrates the official [Claude Code](https://code.claude.com) CLI (and, later, other
agent CLIs like Codex) on *your own machine*, under *your own* subscription login. It gives you:

- **Parallel task board** — kick off many coding tasks at once, each isolated in its own git
  worktree; watch live streams, review diffs, merge.
- **Pipelines / DAGs** — chained stages (plan → implement → review → fix) defined in YAML.
- **Scheduled agents** — cron-style recurring workflows (nightly dep updates, issue triage).
- **Agent teams & debate** — reusable agent definitions, N parallel attempts + a judge.
- **Model tiering** — route easy work to Sonnet, medium to Opus, complex to Fable, per task or
  per pipeline step.

## How it stays within the terms of service

aichip is **process orchestration, not API access**. The compliance model is structural:

1. Every user runs aichip locally and brings their **own installed CLI** and their **own
   subscription login**. aichip never provides, shares, proxies, or resells model access.
2. aichip **never reads, stores, extracts, or forwards credentials** — it does not touch
   `~/.claude`, does not set auth environment variables, and does not proxy network traffic.
3. aichip only spawns the **official binaries found on `PATH`** (e.g. `claude -p
   --output-format stream-json`) and reads their stdout.
4. `aichip doctor` verifies the CLI is installed and logged in **by running it**, never by
   inspecting its config files.

These four invariants are contribution rules. PRs that violate them will not be merged.

## Status

Early development (v0.1 scaffold). See `crates/` for the Rust workspace and `web/` for the
React dashboard.

## Development

```bash
cargo build            # build the Rust workspace
cargo test             # run unit + fixture tests (uses the mock engine; no model usage)
cd web && pnpm install && pnpm dev   # dashboard dev server (proxies to the Rust API)
```

## Workspace layout

- `crates/aichip-shared` — event types, model tiers, API DTOs
- `crates/aichip-engines` — engine adapter trait, Claude Code adapter, mock engine
- `crates/aichip-core` — db, run orchestrator, worktree manager, queue, scheduler
- `crates/aichip-server` — axum REST + WebSocket + MCP permission proxy
- `crates/aichip-cli` — the `aichip` binary (`serve`, `doctor`)
- `web/` — React dashboard
