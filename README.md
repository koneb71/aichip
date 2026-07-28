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

Early development. Task board, agents, teams, chat, pipelines, scheduling, and
prompt attachments all work end to end; see the roadmap below.

## Quick start

```bash
cargo run -p aichip-cli -- doctor   # checks git + the claude CLI are usable
cargo run -p aichip-cli -- serve    # starts the dashboard on http://127.0.0.1:4820
```

The first run downloads and initializes a private Postgres under `~/.aichip/pgdata`,
so there is nothing to install or configure.

## The board

The task board is a real kanban: drag cards between columns and reorder them
within one. Dropping a backlog card into **In Progress** starts its agent —
drag is the verb for "go". A card whose agent is still working refuses to
leave the column until you cancel the run.

Every card has a comment thread. Type `@` to mention an agent by name and it
replies in the thread — after reading the repository, so its answer is grounded
in the code rather than in the question. Mentioned agents can't edit anything
from a comment; they answer, and real changes still go through tasks. Files can
be attached to a card at creation or later from its drawer; the next run sees
them.

Agents keep **memories**: when one finishes a task or answers a mention, a
compact note of what happened is stored and fed into its next runs, so an agent
you work with knows what it has been doing. Memories are visible (and prunable)
in the agent's editor drawer.

## Adding a folder

Point aichip at any folder — it does not need to be a git repository. If it
isn't one, aichip runs `git init` and makes a first commit of whatever is
already there when you add it.

That isn't ceremony. Coding tasks run in an isolated worktree so an agent never
touches your working copy, and that worktree is also what produces the diff you
review before anything is merged back. A repository is the price of that
safety, so aichip creates one rather than asking you to.

A folder occasionally can't have its own repository — most often because it sits
inside another one, where nesting a second repo would confuse every later git
command. Those projects still work, but their tasks **edit the folder directly**:
no worktree, no diff, no review step, and no undo. They're marked
*no version control — edits in place* in the UI, and their cards go straight to
done because there is nothing to review. Full-auto permissions stay refused for
them regardless of project settings, since the worktree that made full-auto safe
isn't there.

## Attachments and file references

Drag an image, PDF, or text file onto the chat composer or the new-task form —
or paste a screenshot straight from the clipboard. The agent reads the file
itself, so a design mock, a spec PDF, or a CSV can go into a prompt instead of
being described in prose.

Attachments are stored under `~/.aichip/attachments/`, **outside your repository**,
and the run is granted read access to them with `--add-dir`. They are never
copied into a task worktree: an untracked file there would show up in
`git status`, and an agent that runs `git add -A` would commit your PDF to the
branch and then to `main` on squash-merge.

Accepted: `png jpg jpeg gif webp pdf txt md csv json log`, up to 10 MB each and
10 per message. The type is decided by the extension and confirmed against the
file's magic bytes, so a renamed binary is rejected. Uploads you abandon are
swept after 24 hours.

To point at code that is already in the repo, type `@` in either composer and
search by filename. Press `:` on a highlighted result (or type it inline) to
pick a specific line or range:

```
compare `web/src/lib/api.ts:120-160` with the screenshot I attached
```

The reference is inserted as a backticked path, which resolves against the
repository root in both chat and task runs.

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
