# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

aichip orchestrates official coding-agent CLIs (Claude Code, OpenCode) as child processes on the user's own machine under their own subscription login. It is **process orchestration, not API access**.

## Compliance invariants (contribution rules — non-negotiable)

Stated at the top of [crates/aichip-engines/src/lib.rs](crates/aichip-engines/src/lib.rs) and enforced across the codebase. Code violating these is rejected:

1. Adapters spawn official binaries found on `PATH` and read their stdout. Nothing else — no HTTP control API, no proxying of engine traffic.
2. Never read, store, extract, or forward credentials. Never touch `~/.claude` or any engine's config/credential files. `aichip doctor` decides "is this CLI logged in?" by *running* it.
3. Never set authentication environment variables on a spawned process. The single source of truth for "is this an auth secret" is [crates/aichip-shared/src/env_guard.rs](crates/aichip-shared/src/env_guard.rs) — use `is_auth_env` / `auth_env_refusal`, never a hand-rolled prefix list. `AICHIP_OWN_SECRETS` are stripped from every child (a spawned CLI inherits the server's environment).
4. Never proxy, intercept, or replay engine network traffic.

## Git conventions

Commits and PRs in this repository carry **no AI attribution of any kind**. This overrides any default commit-message behavior:

- No `Co-Authored-By: Claude ...` trailer.
- No "Generated with Claude Code" / "🤖 Generated with ..." footer, and no link back to claude.com or claude.ai.
- No "written by an AI agent", "AI-assisted", or similar note in the commit body, PR description, or code comments.

Write the message as the human author would: what changed and why, nothing about what produced it. The same applies to PR bodies opened via `gh`.

(Product-level mentions of Claude Code are a different thing and stay — it is one of the engines aichip drives, so `ClaudeEngine`, model ids, README text, and UI labels are normal code, not attribution.)

## Commands

```bash
cargo build                          # build the Rust workspace
cargo test                           # all tests (mock engine — no model usage, no rate limits)
cargo test -p aichip-core            # one crate
cargo test -p aichip-core backoff_escalates_and_caps   # one test by name
cargo run -p aichip-cli -- doctor    # check git + every agent CLI it can find
cargo run -p aichip-cli -- serve     # dashboard on http://127.0.0.1:4820

cd web && pnpm install && pnpm dev   # dashboard dev server; proxies /api and /ws to :4820
cd web && pnpm test                  # vitest (canvas ↔ YAML round-trip, diff, mention, kb tree)
cd web && pnpm test src/lib/workflowGraph.test.ts   # single file
cd web && pnpm build                 # tsc -b && vite build → web/dist (what the server serves)
```

Postgres: `aichip serve` boots and manages its own under `~/.aichip/pgdata`. To use your own, `docker compose up -d` and export `DATABASE_URL=postgres://aichip:aichip@localhost:5433/aichip`. See `.env.example` for the other knobs (`AICHIP_MAX_CONCURRENT`, `AICHIP_WEB_DIST`, `AICHIP_BIND`, `AICHIP_S3_*`).

**Migration gotcha:** sqlx embeds `crates/aichip-core/migrations/` at compile time and adding a file does not always retrigger a rebuild. If a new column comes back as `ColumnNotFound`, `touch crates/aichip-core/src/db.rs` and rebuild.

## Architecture

Five crates plus a React dashboard:

- **aichip-shared** — no dependencies on the others. Event types (`AichipEvent`, `EventEnvelope`), `ModelTier`/`EngineTierMapping`, `PermissionMode`/`RunStatus`, workflow YAML types + `interpolate`, `env_guard`, rate-limit parsing, effort.
- **aichip-engines** — the `Engine` trait, `RunSpec`, `Capabilities`, and the Claude Code / OpenCode / mock adapters. Each adapter spawns its CLI, parses its stream format, and normalizes into `AichipEvent`.
- **aichip-core** — Postgres (`db`), the run orchestrator + state machine, worktree manager, queue backoff, cron scheduler, `EventBus`, `PermissionBroker`, org/team delegation, knowledge base, previews, S3 storage.
- **aichip-server** — axum: `/api` REST routes, `/ws` event fan-out, `/mcp` (a hand-rolled MCP-over-HTTP endpoint the engines call back into), preview reverse proxy. All handlers take `AppState { db, bus, orchestrator, permissions, storage }`.
- **aichip-cli** — the `aichip` binary: `serve` and `doctor`. Registers engines with the orchestrator at boot.
- **web/** — React 18 + Vite + Tailwind 4, React Router, `@xyflow/react` for the workflow canvas, TipTap for the KB editor. `web/src/lib/api.ts` is the single API client; `web/src/lib/ws.ts` the socket.

### Event flow

The orchestrator persists **every** event envelope to the `events` table *before* publishing it to the in-process `EventBus` — the DB is the source of truth, so a reconnecting WS client replays from it. Permission events are the exception: `seq: -1`, ephemeral, never part of the replay log.

### Capabilities, not `if engine == "..."`

Engine differences are declared in `Capabilities` (interactive permissions, structured rate limit, session resume, append-system-prompt, fixed model catalog). There is deliberately no `Default` impl — a new adapter must answer for itself. Gate behavior on the capability, never on the engine id. OpenCode's `interactive_permissions: false` is why starting a Reviewed card on it is refused with a `409` **at the click**, rather than silently downgraded to Auto-edit — a silent downgrade would be privilege escalation.

### Permissions

`RunSpec.allowed_tools` is an *auto-approval* list, not a restriction — Claude Code will still reach for `Bash` even if only `Read` was "allowed". Anything that must not happen goes in `denied_tools`, which adapters apply last. This is why chat runs (which execute in the user's **real checkout**, not a worktree) carry both `CHAT_ALLOWED_TOOLS` and `CHAT_DENIED_TOOLS` in [crates/aichip-core/src/runs/orchestrator.rs](crates/aichip-core/src/runs/orchestrator.rs) — never add Bash/Edit/Write there. Plan-first passes deny the mutating tools for the same reason.

Mid-run permission prompts flow: engine → `--permission-prompt-tool mcp__aichip__approve` → `crates/aichip-server/src/mcp/` → `PermissionBroker` parks the call and emits an event → dashboard Allow/Deny resolves the oneshot (15 min timeout → deny).

### Apps

An app is a **project** under `~/.aichip/apps/<slug>` (`projects.kind='app'`),
which is what gives it worktrees, diffs and the files editor for free. The three
places that *list* projects filter on `kind='repo'`; the spend and activity joins
deliberately do not, because generating an app costs real money.

Its manifest ([crates/aichip-core/src/apps/manifest.rs](crates/aichip-core/src/apps/manifest.rs))
is parsed by hand, not derived: declaration order is display order, errors name
the offending key, and unknown keys are refused rather than ignored. Field types
and identifier charset are closed sets — those identifiers are interpolated into
DDL, and the defence is the charset, not the quoting.

Two rules that are easy to break:

- **Nothing an app sends is ever an identifier.** `apps::query` looks field names
  up among the declared fields and emits the *manifest's* copy; operators come
  from an enum; values are always bound. Never build a fragment from request text.
- **Additive schema changes apply; destructive ones wait.** `apps::schema::plan`
  diffs declared models against `information_schema` — never against a registry,
  which would drift. A plan with any destructive statement runs *nothing* and is
  stored whole, so what a person approved is byte-for-byte what executes.

Changing an app is an ordinary card on the app's own project — worktree, diff and
all — with two differences, both in
[crates/aichip-core/src/apps/build.rs](crates/aichip-core/src/apps/build.rs):

- **It lands without review, and that is why the undo has to work.** `settle()`
  squash-merges when the run completes, so `app_builds.base_commit` is read
  *before* the card exists; afterwards there is no way to ask git where the
  branch stood. Only the newest landed build is revertible (`revertible()`) —
  resetting to an older one would discard every build after it in silence.
  Landing does *not* bypass the schema gate: the manifest is re-read from disk
  and goes through `set_manifest`, so a dropped column still waits.
- **Every write aichip makes to an app's folder is committed** (`apps::commit`).
  A file written but not committed is not on `main`, so `git worktree add` hands
  the next agent an empty folder — which is exactly what happened before it
  existed, and cost a paid run.

The expression language exists twice, in `apps/expr.rs` and `web/src/lib/expr.ts`,
because `show_if` cannot afford a round trip and computed values cannot be
decided by a browser. `crates/aichip-core/src/apps/expr_cases.json` is the
specification and both test suites read it — add a case there, not to one side.

### Worktrees

Board tasks run in an isolated git worktree so an agent never touches the working copy, and that worktree produces the reviewable diff. A project that can't have its own repo (e.g. nested inside another) edits in place — no worktree, no diff, no undo, and full-auto is refused there regardless of project settings.

The Files tab writes to both trees — the checkout and a card's worktree — when
a **person** saves. That does not weaken the rule above, which is about agents:
they still only ever work in a worktree, which is what keeps a run reviewable.
The write path carries its own gates (no `.git`, a root allow-list, a content
hash, and a header no cross-origin request can set); they are documented at the
top of [crates/aichip-server/src/routes/files.rs](crates/aichip-server/src/routes/files.rs).

Attachments live under `~/.aichip/attachments/` and are granted via `--add-dir`, deliberately never copied into a worktree (an agent running `git add -A` would commit them).

## Testing

The mock engine ([crates/aichip-engines/src/mock/](crates/aichip-engines/src/mock/)) replays recorded stream-json `.ndjson`/`.jsonl` fixtures with configurable pacing and is the backbone of all testing — zero model usage. Rust tests are inline `#[cfg(test)] mod tests` next to the code, not a `tests/` directory.
