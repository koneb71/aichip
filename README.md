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

Early development. Task board, agents, teams, chat, pipelines, and scheduling all work
end to end; see the roadmap below.

## Quick start

```bash
cargo run -p aichip-cli -- doctor   # checks git + the claude CLI are usable
cargo run -p aichip-cli -- serve    # starts the dashboard on http://127.0.0.1:4820
```

The first run downloads and initializes a private Postgres under `~/.aichip/pgdata`,
so there is nothing to install or configure.

## Workflows

Build workflows on a canvas — drag between node handles to say "run after",
click a node to edit its prompt, model, agent, and fan-out. The canvas is a view
over YAML, which stays the source of truth: flip to the YAML tab any time, or
commit files to `.aichip/workflows/` in your repo and press **Sync from repo**.
(Canvas edits regenerate the YAML, so comments in a hand-written file don't
survive a round trip through the canvas.)

```yaml
name: nightly-dep-audit
on: { schedule: "0 3 * * *" }     # standard 5-field cron
defaults: { permission_mode: auto_edit }
steps:
  - id: audit
    model: easy                    # easy | medium | complex → Sonnet | Opus | Fable
    prompt: "List outdated dependencies and flag any with security advisories."
  - id: fix
    needs: [audit]
    model: medium
    session: continue              # resume the previous step's session
    prompt: "Upgrade the safe ones:\n{{ steps.audit.output }}"
```

Steps run in dependency order and outputs flow forward. Add
`strategy: { parallel: 3, isolated_worktrees: true }` to a step and it fans out into
independent attempts in separate worktrees — a step that `needs` it then sees every
attempt via `{{ steps.<id>.outputs }}`, which is how debate-with-a-judge works.

Scheduled workflows fire from the cron in `on.schedule`. If the machine was asleep past
a scheduled time, the missed run is skipped by default rather than stampeding on wake;
set `catch_up` to `run_once` on the workflow to run one catch-up instead.

## Organizations

An organization is a team with a manager. Give it a goal and the manager reads
your repository, splits the work into briefed assignments, and delegates each to
the specialist best suited to it. Specialists work one at a time in a shared
worktree — like real teammates on one codebase, so you get a single coherent
diff instead of branches to merge.

They talk while they work, and you watch it happen:

- `post_message` — tell the team what you're doing or what you found
- `read_messages` — catch up on what teammates have said
- `ask_manager` — escalate a decision; this blocks the specialist while the
  manager answers from the context of the plan it wrote (capped per assignment,
  so nobody stalls forever)

The live view shows the roster with each teammate's state, the conversation as
it happens, and the assignment board filling in. Build one on the **Teams** page
by choosing the Organization pattern, picking a manager, and giving each
specialist a role.

**Board tasks can be assigned to a team**, not just a single agent. Pick a team
in the "Assign to" dropdown and the task runs as that team — an organization
delegates it, a pipeline or debate team runs its pattern. Either way the work
happens in the task's own worktree, so the card lands on Review and you diff
and squash-merge it exactly like a solo task. Open the card and hit **Open team
room** to watch or replay the conversation behind it.

## Database

`aichip serve` manages its own Postgres by default. To use your own instead:

```bash
docker compose up -d
export DATABASE_URL=postgres://aichip:aichip@localhost:5433/aichip
cargo run -p aichip-cli -- serve
```

See `.env.example` for the other knobs.

### Why isn't the app containerized?

aichip works by spawning *your* `claude` CLI under *your* login. That login lives on your
machine, so the server runs on your machine too — containerizing it would mean mounting
your credentials into a container for no benefit. The database is the part that gains
from a container, so that's the part compose provides.

## Development

```bash
cargo build            # build the Rust workspace
cargo test             # unit + fixture tests (mock engine; no model usage, no rate limits)
cd web && pnpm install && pnpm dev   # dashboard dev server (proxies to the Rust API)
cd web && pnpm test    # canvas ↔ YAML round-trip tests
```

Adding a migration under `crates/aichip-core/migrations/` does not always
retrigger a rebuild, because sqlx embeds them at compile time. If a new column
comes back as `ColumnNotFound`, `touch crates/aichip-core/src/db.rs` and rebuild.

## Workspace layout

- `crates/aichip-shared` — event types, model tiers, API DTOs
- `crates/aichip-engines` — engine adapter trait, Claude Code adapter, mock engine
- `crates/aichip-core` — db, run orchestrator, worktree manager, queue, scheduler
- `crates/aichip-server` — axum REST + WebSocket + MCP permission proxy
- `crates/aichip-cli` — the `aichip` binary (`serve`, `doctor`)
- `web/` — React dashboard
