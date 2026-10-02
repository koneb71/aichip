# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Eren orchestrates official coding-agent CLIs (Claude Code, OpenCode, Codex — plus Ollama and LM Studio models driven through OpenCode) as child processes on the user's own machine under their own subscription login. It is **process orchestration, not API access**.

## Compliance invariants (contribution rules — non-negotiable)

Stated at the top of [crates/eren-engines/src/lib.rs](crates/eren-engines/src/lib.rs) and enforced across the codebase. Code violating these is rejected:

1. Adapters spawn official binaries found on `PATH` and read their stdout. Nothing else — no HTTP control API, no proxying of engine traffic.
2. Never read, store, extract, or forward credentials. Never touch `~/.claude` or any engine's config/credential files. `eren doctor` decides "is this CLI logged in?" by *running* it.
3. Never set authentication environment variables on a spawned process. The single source of truth for "is this an auth secret" is [crates/eren-shared/src/env_guard.rs](crates/eren-shared/src/env_guard.rs) — use `is_auth_env` / `auth_env_refusal`, never a hand-rolled prefix list. `EREN_OWN_SECRETS` are stripped from every child (a spawned CLI inherits the server's environment) — which is why every process starts through `env_guard::command`, never `Command::new`. `clippy.toml` and a source-scanning test in `env_guard.rs` both refuse the latter.
4. Never proxy, intercept, or replay engine network traffic.

## Git conventions

Commits and PRs in this repository carry **no AI attribution of any kind**. This overrides any default commit-message behavior:

- No `Co-Authored-By: Claude ...` trailer.
- No "Generated with Claude Code" / "🤖 Generated with ..." footer, and no link back to claude.com or claude.ai.
- No "written by an AI agent", "AI-assisted", or similar note in the commit body, PR description, or code comments.

Write the message as the human author would: what changed and why, nothing about what produced it. The same applies to PR bodies opened via `gh`.

(Product-level mentions of Claude Code are a different thing and stay — it is one of the engines Eren drives, so `ClaudeEngine`, model ids, README text, and UI labels are normal code, not attribution.)

## Commands

```bash
cargo build                          # build the Rust workspace
cargo test                           # all tests (mock engine — no model usage, no rate limits)
cargo test -p eren-core            # one crate
cargo test -p eren-core backoff_escalates_and_caps   # one test by name
cargo run -p eren-cli -- doctor    # check git + every agent CLI it can find
cargo run -p eren-cli -- serve     # dashboard on http://127.0.0.1:4820

cd web && pnpm install && pnpm dev   # dashboard dev server; proxies /api and /ws to :4820
cd web && pnpm test                  # vitest (canvas ↔ YAML round-trip, diff, mention, kb tree)
cd web && pnpm test src/lib/workflowGraph.test.ts   # single file
cd web && pnpm build                 # tsc -b && vite build → web/dist (what the server serves)
```

Postgres: `eren serve` boots and manages its own under `~/.eren/pgdata`. To use your own, `docker compose up -d` and export `DATABASE_URL=postgres://eren:eren@localhost:5433/eren`. See `.env.example` for the other knobs (`EREN_MAX_CONCURRENT`, `EREN_WEB_DIST`, `EREN_BIND`, `EREN_S3_*`).

**Migration gotcha:** sqlx embeds `crates/eren-core/migrations/` at compile time and adding a file does not always retrigger a rebuild. If a new column comes back as `ColumnNotFound`, `touch crates/eren-core/src/db.rs` and rebuild.

## Architecture

Five crates plus a React dashboard:

- **eren-shared** — no dependencies on the others. Event types (`ErenEvent`, `EventEnvelope`), `ModelTier`/`EngineTierMapping`, `PermissionMode`/`RunStatus`, workflow YAML types + `interpolate`, `env_guard`, rate-limit parsing, effort.
- **eren-engines** — the `Engine` trait, `RunSpec`, `Capabilities`, and the Claude Code / OpenCode / Codex / Ollama / LM Studio / mock adapters. Each adapter spawns its CLI, parses its stream format, and normalizes into `ErenEvent`.

  The last two are a different shape and the reason is written at the top of [crates/eren-engines/src/local/mod.rs](crates/eren-engines/src/local/mod.rs): a local runtime serves a model but holds no tools, so `LocalEngine` **delegates to the OpenCode binary** with the provider declared and the model resolved from what `ollama list` / `lms ls --json` actually report. Invariant 1 still holds — it spawns official binaries from `PATH` and reads their stdout. `eren_core::local_models` is the older HTTP discovery used by the settings page; an adapter must not use it.
- **eren-core** — Postgres (`db`), the run orchestrator + state machine, worktree manager, queue backoff, cron scheduler, `EventBus`, `PermissionBroker`, org/team delegation, knowledge base, previews, S3 storage.
- **eren-server** — axum: `/api` REST routes, `/ws` event fan-out, `/mcp` (a hand-rolled MCP-over-HTTP endpoint the engines call back into), preview reverse proxy. All handlers take `AppState { db, bus, orchestrator, permissions, storage }`.
- **eren-cli** — the `eren` binary: `serve` and `doctor`. Registers engines with the orchestrator at boot.
- **web/** — React 18 + Vite + Tailwind 4, React Router, `@xyflow/react` for the workflow canvas, TipTap for the KB editor. `web/src/lib/api.ts` is the single API client; `web/src/lib/ws.ts` the socket.

### Event flow

The orchestrator persists **every** event envelope to the `events` table *before* publishing it to the in-process `EventBus` — the DB is the source of truth, so a reconnecting WS client replays from it. Permission events are the exception: `seq: -1`, ephemeral, never part of the replay log.

### Capabilities, not `if engine == "..."`

Engine differences are declared in `Capabilities` (interactive permissions, structured rate limit, session resume, append-system-prompt, fixed model catalog). There is deliberately no `Default` impl — a new adapter must answer for itself. Gate behavior on the capability, never on the engine id. OpenCode's `interactive_permissions: false` is why starting a Reviewed card on it is refused with a `409` **at the click**, rather than silently downgraded to Auto-edit — a silent downgrade would be privilege escalation.

### Agent status

An agent is `active`, `paused`, `retired` or `pending_approval` (`eren_core::agents`). Every function that inserts a run asks `agents::assert_can_run` (or its team / workflow-step form) **before** the insert; a source-scanning test fails the build for one that does not, unless it is on the short list of runs with no agent. A team run and a workflow ask again at each assignment, so a pause stops the agent's *next* piece of work wherever it was coming from. A paused agent can still be assigned cards; a retired one cannot, and is hidden from every picker. Deleting an agent that anything references retires it instead.

### Org chart

`agents.reports_to` makes a workspace's agents a tree, kept one by `org_chart::set_manager` — the only writer — under a per-workspace advisory lock: same workspace, no cycle, depth ≤ `MAX_DEPTH`, no retired manager. Retiring (or deleting) a manager lifts its reports to its own manager in the same transaction. Delegation flows down (a manager pass is told its reports, and `create_task` from a pass whose agent has reports may bind only agents in its subtree — `may_delegate`); trouble flows up (`wake::ESCALATES` kinds also wake the manager routine of the card's agent's manager). A workspace that never draws a chart works exactly as before.

### Goals

`eren_core::goals`: a per-workspace tree (one writer of `parent_id`, an advisory lock, no cycle, depth ≤ `MAX_DEPTH`). Cards and manager routines point at the goal they serve; a card split from an epic inherits the epic's goal by a `BEFORE INSERT` trigger (epics are split in more than one place), and a card a manager pass files takes the routine's goal unless it names one. Every card run gets a fenced, capped "Why this matters" — the goal chain top-down plus the epic — appended after the brief, Brain and skill; a manager pass gets the active goals with progress. Progress is counted from the cards in a goal's subtree on every read, never stored. Deleting a goal moves its children and cards up to its parent.

### Heartbeats

`eren_core::heartbeat`: an agent with `heartbeat_secs` (a person's standing decision) beats on a timer — claimed with a compare-and-set on `last_heartbeat_at` — and early through `wakeups` with an `agent_id` target (a card assigned to it, one of its cards unblocked). A beat does at most one thing: nothing if paused or already working (one card at a time unless `max_concurrent` says more); fire the manager routine it runs (same cooldown and daily cap as wakes); or start its next unblocked backlog card **through `start_card`**, so every gate the Start button meets applies. Idle beats call no model. Every beat is logged to `heartbeats`, 500 per agent.

### Budgets

A budget policy (`eren_core::budgets`) covers the machine, a workspace, a project, an agent, a team or a routine over a calendar day, week or month, and caps any of dollars, output tokens and runs. It bites in four places, cheapest first: every door that starts work refuses a spent scope with a 409 naming the policy (`budgets::check`, mapped by `run_refused`); `claim_next` holds a queued run whose scope is spent (`queue.hold_reason` / `held_by`) and claims the next instead; a team or workflow starts nothing more between steps; and on a `stop` policy a run crossing a token cap is interrupted mid-stream. That stop measures each run against the headroom left when it started (`budgets::token_headroom`), so runs in flight together can overrun the cap by their combined size; a run is claimed (and counted against a run cap) when it leaves the queue, which is why `budget_allows_more` and a run back from a rate limit do not count it twice. **Dollars cannot stop a run midway** — no engine prices a run until it ends — and an engine with `Capabilities::reports_cost == false` (Codex) is only visible to token caps, so its runs stay unpriced (`cost_usd` NULL) rather than recorded as $0. Warnings and exceeded notices are written once per policy per window (`budget_incidents`), which is what keeps notifications from repeating. Agents also carry their own limits (`max_concurrent`, `max_daily_runs`, `cooldown_secs`), checked in the same claim step; with no policies and no limits, claiming is unchanged.

### Inbox

Everything waiting on a person is one list, `eren_core::inbox::list` — a query over the rows that already say they are waiting (parked plans, chat questions and plans, pending app schema plans, KB revisions, proposed preview recipes), never an index of its own, which would drift. Only what had no row got one (migration 0078): `permission_requests` (written by the broker through `RunGate::record`/`close`; `recover_orphans` marks open ones `expired`, and the inbox offers to resume those runs), `run_questions` (a card agent's `ask_person`; the answer returns as `FollowUp::Answer` in the same worktree and session, fenced as `fence::ANSWER_*`), and `decisions` (an agent's `propose_decision`, a closed `decisions::Effect`). Every answer goes through the function the thing's own button calls — the transitions live in `eren_core::approvals`, and `routes/inbox.rs` dispatches to them, the broker, or the route handler itself. **No agent toolbox may resolve an inbox item**: an agent proposes, a person decides; `run_tools`' test forbids `resolve`, `approve` and `decide` in tool names.

### Audit log

`audit_log` (migration 0079) is append-only and has exactly one writer, `eren_core::audit::record` — a source scan fails the build for any other `INSERT`/`UPDATE`/`DELETE` on it, and the only delete is its retention `prune` (agent and system rows after 90 days; API rows kept). Three things feed it: the server's `audit_layer` on every mutating `/api` request (route template, path ids and status — **never the body**; the few read-only POSTs are listed in `audit_layer::QUIET`, each with its reason), the three MCP dispatches (tool name and outcome — **never the input**), and Eren's own actions (routine fires, sweeps). It records `api`, not "a person": there is no login, and any local process can call the API. A card's merged story is `GET /tasks/{id}/timeline`.

### Config revisions

An edit to an agent, team, routine (manager included), skill, project checks, budget policy or the attention setting keeps the row it replaced in `config_revisions` (migration 0080, last 20 per entity) — `eren_core::revisions::keep`, called by the entity's own update and delete handlers; `revisions::tests::every_writer_keeps_a_revision` names each one and fails for a writer added without it. Rows are read generically (`to_jsonb`) from a table chosen by the closed `EntityKind` enum, never from request text. **A restore is an edit**: `routes/revisions.rs` maps the snapshot onto that entity's own update body and calls its own handler, so validation, the write header, the single writer of `project_checks` and the audit entry all apply, and the restore is itself a revision.

### Permissions

`RunSpec.allowed_tools` is an *auto-approval* list, not a restriction — Claude Code will still reach for `Bash` even if only `Read` was "allowed". Anything that must not happen goes in `denied_tools`, which adapters apply last. This is why chat runs (which execute in the user's **real checkout**, not a worktree) carry both `CHAT_ALLOWED_TOOLS` and `CHAT_DENIED_TOOLS` in [crates/eren-core/src/runs/orchestrator.rs](crates/eren-core/src/runs/orchestrator.rs) — never add Bash/Edit/Write there. Plan-first passes deny the mutating tools for the same reason.

Mid-run permission prompts flow: engine → `--permission-prompt-tool mcp__eren__approve` → `crates/eren-server/src/mcp/` → `PermissionBroker` parks the call and emits an event → dashboard Allow/Deny resolves the oneshot (15 min timeout → deny).

A card's run also gets Eren's own toolbox on `/mcp/run/{run_id}` ([crates/eren-server/src/mcp/run_tools.rs](crates/eren-server/src/mcp/run_tools.rs)): `comment`, `report_blocker`, `search_kb`, `read_article`, `recall`. What a run is offered is read from its row (a planning or summary pass only reads), and every MCP endpoint refuses calls once its run has ended. These tools pass `approve` without asking a person, so **nothing added there may merge, start a run, or write settings or check commands**.

### Apps

An app is a **project** under `~/.eren/apps/<slug>` (`projects.kind='app'`),
which is what gives it worktrees, diffs and the files editor for free. The three
places that *list* projects filter on `kind='repo'`; the spend and activity joins
deliberately do not, because generating an app costs real money.

Its manifest ([crates/eren-core/src/apps/manifest.rs](crates/eren-core/src/apps/manifest.rs))
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
[crates/eren-core/src/apps/build.rs](crates/eren-core/src/apps/build.rs):

- **It lands without review, and that is why the undo has to work.** `settle()`
  squash-merges when the run completes, so `app_builds.base_commit` is read
  *before* the card exists; afterwards there is no way to ask git where the
  branch stood. Only the newest landed build is revertible (`revertible()`) —
  resetting to an older one would discard every build after it in silence.
  Landing does *not* bypass the schema gate: the manifest is re-read from disk
  and goes through `set_manifest`, so a dropped column still waits.
- **Every write Eren makes to an app's folder is committed** (`apps::commit`).
  A file written but not committed is not on `main`, so `git worktree add` hands
  the next agent an empty folder — which is exactly what happened before it
  existed, and cost a paid run.

The expression language exists twice, in `apps/expr.rs` and `web/src/lib/expr.ts`,
because `show_if` cannot afford a round trip and computed values cannot be
decided by a browser. `crates/eren-core/src/apps/expr_cases.json` is the
specification and both test suites read it — add a case there, not to one side.

### Worktrees

Board tasks run in an isolated git worktree so an agent never touches the working copy, and that worktree produces the reviewable diff. A project that can't have its own repo (e.g. nested inside another) edits in place — no worktree, no diff, no undo, and full-auto is refused there regardless of project settings.

The Files tab writes to both trees — the checkout and a card's worktree — when
a **person** saves. That does not weaken the rule above, which is about agents:
they still only ever work in a worktree, which is what keeps a run reviewable.
The write path carries its own gates (no `.git`, a root allow-list, a content
hash, and a header no cross-origin request can set); they are documented at the
top of [crates/eren-server/src/routes/files.rs](crates/eren-server/src/routes/files.rs).

### Checks and follow-ups

A **follow-up** (`runs/follow_up.rs`) is a run that goes back into a card's existing worktree to act on something said about its diff — a review note, failing checks — so the fix lands in the same diff. It records its note in `runs.review_comment_id`, never `comment_id`, which every reader takes to mean "a comment reply".

**Checks** (`eren_core::checks`) are a project's own test/lint commands, run in a card's worktree after an agent finishes. Two rules that are easy to break:

- **Only `routes/checks.rs` writes `project_checks`.** It holds shell commands this machine runs; a test fails if any other file writes it. Never put a check command on a row an agent, an importer or an app build can write.
- **Checks start unasked only after a Full Auto run — or where the project's review policy sets `run_checks_after_every_run`.** They execute code the agent may have edited, so otherwise a person clicks "Run checks" — that click is the consent, and the policy switch is the standing form of it. The same goes for the bounded auto-fix.

**Review policy** (`eren_core::review`, written only by `routes/reviews.rs` — a test fails otherwise) decides what a person's Merge requires: an agent review's approval, passing checks, a green pull request, each of the *latest* work (`last_work`) — an older green or approval does not count. Merge stays a person's click; an unmet gate answers `409 {kind:"gate", unmet}`, and `{force, note}` merges anyway with the note on the card and in the audit log. The reviewer is a `peer_review` follow-up: read-only like a planning pass, run as the reviewer agent on its engine, refused for the card's own author or an engine without `Capabilities::enforces_denied_tools`. Its verdict comes back only through `submit_review`; a review that ends without one is recorded as changes requested (fail closed). `settle_review` is idempotent — called after every completed run and after checks settle, it starts a review only when nothing of the card is live and no verdict covers the latest work — and the loop is bounded: one `ReviewNote` fix per round, then at `max_rounds` the card waits in the inbox. A person's "Review again" gets one round past the cap.

**Handoff** (`eren_core::handoff`): reassigning a running card is refused unless it carries a note — then the request is recorded on the card, the running agent is stopped, and `settle` hands over only once no run of the card is live *and* the old run's `execute` has returned (`Orchestrator::is_executing`, an in-memory registry held by a drop guard): its post-work touches the same worktree, so a terminal status alone is not enough. The new agent continues in the same worktree through `FollowUp::Handoff` (no old session), or starts fresh with the note beside the brief when there is no worktree yet. The swap is a single claiming `UPDATE`, so the immediate attempt and the scheduler sweep cannot both start it.

**Wakes** (`eren_core::wake`): news a project's manager would want before its next scheduled pass — a card landed, unblocked or failed, checks or review rounds exhausted, a question asked, a run stalled. `wake::raise` writes a row only for a manager that ticked that kind (`routines.on_events`, empty by default — an early pass costs a run), coalesced one open row per (target, kind, card). `drain_wakeups` fires through `routines::fire("wake")` only when the manager's thread is idle (checked before firing, so news is kept, not dropped), its cooldown has passed and it has early passes left today. Every manage pass, early or scheduled, opens with a fenced "Since your last pass" and consumes it only once its turn queued. A raise never fails the thing that raised it.

**Reaper** (`eren_core::reaper`, each scheduler tick): a run whose row says starting/running/waiting but that this process is not executing (`is_executing` false for over `LOST_AFTER`) is failed as lost — always on, since nothing else could ever finish it. Stopping a run for silence (no event for N minutes, never while it waits on a person) and resuming stopped runs are both opt-in ("Unattended runs", a revisioned setting). Auto-resume goes through `runs::resume::resume_dead_run` — the Resume button's own door, which the route now calls too — once per stopped run (`auto_resumed_at`) and at most twice along a `resumed_from` chain. Every stall is said on the card, raised as a `stalled` wake and sent through attention.

**Merge conflicts** are met on the card's branch, never the person's checkout: "Update from main" runs `update_from_base`, which merges the base into the card's `eren/…` branch inside its worktree and, on conflict, leaves the merge in progress for a `conflict` follow-up. `commit_worktree` refuses while conflict markers remain (and concludes the merge unconditionally once they're gone), and `squash_merge` refuses any diff that adds them — so markers can never land. A card's diff is measured from `merge-base`, not the base's tip.

**Landing** (`eren_core::landing`): a card blocked by another waits for it to reach *done*. Six things write `done` and share no code path, so the seam is `tasks.landed_at`, set once by whichever notices first. A writer of done calls `orchestrator.landed(task_id)` (a no-op if the card is not done or already landed); `settle_landings` sweeps every scheduler tick for the ones that do not. A dependent with `start_when_unblocked` starts through `start_card` — the Start button's vet and door — and every other dependent gets a note and an `unblocked` attention event.

Attachments live under `~/.eren/attachments/` and are granted via `--add-dir`, deliberately never copied into a worktree (an agent running `git add -A` would commit them).

## Testing

The mock engine ([crates/eren-engines/src/mock/](crates/eren-engines/src/mock/)) replays recorded stream-json `.ndjson`/`.jsonl` fixtures with configurable pacing and is the backbone of all testing — zero model usage. Rust tests are inline `#[cfg(test)] mod tests` next to the code, not a `tests/` directory.

Tests of SQL live in an inline `mod db_tests` and start with `let Some(t) = testdb::fresh().await else { return };` ([crates/eren-core/src/testdb.rs](crates/eren-core/src/testdb.rs)): each gets its own migrated database on the server `DATABASE_URL` names, an orchestrator with the mock engine if it asks, and skips when `DATABASE_URL` is unset. CI runs them against a Postgres service. `DATABASE_URL=postgres://eren:eren@localhost:5433/eren cargo test` runs them against `docker compose up -d`.
