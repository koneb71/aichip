# aichip

**A local-first multi-agent workflow platform for coding agents — no API keys.**

aichip orchestrates official coding-agent CLIs — [Claude Code](https://code.claude.com) and
[OpenCode](https://opencode.ai) today — on *your own machine*, under *your own* subscription
login. It gives you:

- **Parallel task board** — kick off many coding tasks at once, each isolated in its own git
  worktree; watch live streams, review diffs, merge.
- **Pipelines / DAGs** — chained stages (plan → implement → review → fix) defined in YAML.
- **Scheduled agents** — cron-style recurring workflows (nightly dep updates, issue triage).
- **Agent teams & debate** — reusable agent definitions, N parallel attempts + a judge.
- **Model tiering** — route easy work to a fast model and hard work to a strong one, per task
  or per pipeline step. The mapping is *per engine*, because "medium" cannot name one model
  globally: OpenCode has never heard of `claude-opus-5`.
- **More than one engine** — pick which CLI runs a card, a chat, a team or a single workflow
  step, and pit two against each other on the same brief in a bake-off. An engine that isn't
  installed is simply not offered, and one that can't honour a permission mode says so before
  you start rather than failing forty minutes in.

## How it stays within the terms of service

aichip is **process orchestration, not API access**. The compliance model is structural:

1. Every user runs aichip locally and brings their **own installed CLI** and their **own
   subscription login**. aichip never provides, shares, proxies, or resells model access.
2. aichip **never reads, stores, extracts, or forwards credentials** — it does not touch
   `~/.claude`, does not set auth environment variables, and does not proxy network traffic.
3. aichip only spawns the **official binaries found on `PATH`** (e.g. `claude -p
   --output-format stream-json`, `opencode run --format json`) and reads their stdout. No
   engine has an HTTP control API in the loop.
4. `aichip doctor` verifies each CLI is installed and logged in **by running it**, never by
   inspecting its config files. Where a CLI can name its providers (`opencode providers
   list`), aichip shows the **name and auth type only** — never a credential.

These four invariants are contribution rules. PRs that violate them will not be merged.

## Status

Early development. Task board, agents, teams, chat, pipelines, scheduling, and
prompt attachments all work end to end; see the roadmap below.

## Quick start

```bash
cargo run -p aichip-cli -- doctor   # checks git + every agent CLI it can find
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

### Plan first

A card can be set to **plan first**. The agent reads the code and writes down
what it means to do — what it found, which files it will touch, what it is
leaving alone, what it had to guess — then stops. Nothing has changed yet.

You get three answers, and the middle one is why this exists:

- **Approve** — work starts from the plan.
- **Edit, then approve** — rewrite the plan in place and start from *your*
  version. When a plan is 90% right, fixing the line beats paying for another
  planning pass to fix it for you.
- **Ask for changes** — send it back with a note; the next pass gets your
  feedback alongside what it proposed.

The planning pass is genuinely read-only: `Edit`, `Write`, `Bash` and friends
are *denied*, not merely left off the allow-list. That distinction is load
bearing — Claude Code's `--allowedTools` pre-approves rather than restricts, so
a planning pass "allowed" only `Read` will still reach for `Bash` unless told
it cannot.

The work pass resumes the planning session, so the agent keeps everything it
learned reading the code. When you edited the plan it is told so explicitly,
because it is resuming a conversation in which it remembers proposing something
else, and would otherwise follow its own memory over the text in front of it.

While parked the run holds no queue slot: planning finishes, the run waits, and
approving re-queues it.

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

The Files tab is an editor and does save — to your checkout, and to a card's
worktree so you can fix up what an agent produced before merging it. That is
you writing your own files, deliberately; the guarantee above is about agents,
and it is unchanged.

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

## Knowledge base

A wiki, not a folder of documents. Pages have **a place, an address, and a
memory**:

- **A place.** Pages nest under each other in a tree, grouped into **spaces** —
  and a space is a repository, because the pages worth writing are about a
  codebase. Pages that belong to no one repo live in *General*.
- **An address.** `/knowledge/:pageId` is a real route with a read view,
  breadcrumbs, a contents list, child pages, and "linked from". Type `@` in the
  editor to link another page; the backlink appears on the other end.
- **A memory.** Every version is kept. The history shows what changed, when, and
  whether a person or an agent wrote it — including the versions that were
  turned down, because "what did we decide not to do" is usually the interesting
  question.

Editing has no Save button: typing saves. Every save carries the revision it
started from, so if the page moved under you the server refuses rather than
overwriting, and you get a diff and a choice instead of a lost afternoon.

### Agents propose; they never overwrite

This wiki has two kinds of writer, and that is the unusual part. An agent can
read your repository and write documentation for it — but **an agent's write is
always a proposal**. The live page is untouched until a person accepts it.

You get the proposal as a diff, and three answers: accept, *accept and edit*, or
discard with a note. If you edited the page while the agent was working, the
banner says so and names the revision the agent actually read.

The diff is over the page's **text**, never its HTML — two model passes over
identical prose emit different markup, and a diff that claims every line changed
is a diff nobody reads, which turns review into rubber-stamping.

### What agents read

Attach a page to a card and its text is folded into that run's prompt, so
attaching one is how you say "read the runbook before you touch anything". You
can also reference a page in a single comment, which reaches the reply without
pinning it to the card.

Those bodies reach a process holding Edit, Write and Bash, and *another agent
may have written them* — so the prompt fences each page, states plainly that the
enclosed text is reference material rather than instructions, and says whether a
human has published the page or it is still an unreviewed draft. Titles are
escaped and bodies cannot close their own fence.

### Storage and search

Page text lives in Postgres and is fully searchable — titles, summaries and
bodies. Images and files pasted into a page go to **MinIO** (or any
S3-compatible endpoint), so a 4 MB screenshot doesn't end up in every database
backup:

```bash
docker compose up -d minio
export AICHIP_S3_ENDPOINT=http://127.0.0.1:9100
export AICHIP_S3_ACCESS_KEY=aichip
export AICHIP_S3_SECRET_KEY=aichip-dev-secret
```

The bucket is created on boot. Without these variables the wiki still works —
you just can't attach files, and the upload endpoint says so.

Bodies are sanitised **on write**, not on render: editor HTML is stored and
served back to other readers, which is the textbook stored-XSS shape, and a
string that is only safe when every reader remembers to clean it will meet the
reader who forgets. Embeds are allowed from a short host allowlist; an
`<iframe>` pointing anywhere else does not survive being saved.

The editor is [TipTap](https://tiptap.dev) — MIT, self-hosted, no account and no
licence key. It emits HTML, which is why swapping it changed nothing behind it:
the same sanitiser, the same text projection, the same diff, the same search
index. A markdown-first editor would have meant rewriting all four.

## What it costs, and spending less

The binding constraint here is a subscription rate limit you cannot see, so
aichip keeps what your CLI says about its own usage rather than discarding it.
It asks Anthropic nothing, holds no credential, and prices nothing itself: a
figure here is one the binary printed.

**Where the tokens went.** The activity page breaks the window down five ways
— by project, engine, model, tier, and which *feature* spent it. The last one
is the useful one, because the dearest line is usually a pattern rather than a
project: a bake-off is several runs on one brief, a debate team is several
attempts plus a judge, a plan-first card is two passes. None of that shows up
in a per-run cost.

**Cache hit rate** leads the panel. Cached input costs a fraction of fresh
input, so the share of your tokens served from cache — not the token count —
is what separates a cheap run from an expensive one. It reads `—`, never
`0%`, when nothing has been sent: a fresh install has not proven its cache
broken.

The totals name their own gaps. Runs whose engine never reported a price are
counted separately instead of being folded into a number that looks complete,
and runs that ended without a final tally are marked as carrying estimates.
Both used to show as `$0` — a run you cancelled after twenty minutes cost real
money and reported nothing, which also meant the daily cap under-counted it.

**A daily cap** in dollars stops the queue when the day's spend passes it, and
clears on its own at midnight rather than needing to be resumed.

### Auto tier

A card's tier can be set to **Auto**, and aichip picks per run.

This is worth having because `Medium` is the default and maps to Opus, so
every card nobody thought about runs on the dearest ordinary model. Auto is
opt-in and never the default — a router that switched itself on for everyone
would be making exactly the choice it exists to surface.

The rules are ordered and first-match-wins, not a score, because a score
cannot be explained to the person whose card it just routed:

- A **retry never routes below what already failed.** Rerunning a failure on a
  cheaper model pays twice to lose twice.
- **Planning gets the strong model; carrying out an approved plan gets a
  cheaper one.** This is the biggest honest saving — the judgment already
  happened and you approved it.
- A **long brief with several attachments or runbooks** is a briefing, not a
  chore, and goes up.
- A **short brief with nothing attached** goes down.
- Anything else stays at Medium, exactly as before.

Two rules are deliberately missing. Historical project cost is *not* an input:
it measures the model that was used, so routing on it closes a loop where
cheap runs keep justifying the cheap tier. And shortness alone never buys the
cheap tier — a short brief is as likely to be under-specified as simple, and a
brief below a floor buys nothing at all.

Every automatic choice is recorded on the run before it starts and shown on
the card: *Auto → easy: a short brief with nothing attached*. That isn't
decoration. aichip refuses to quietly downgrade a Reviewed card on an engine
that cannot ask, and a router that changed which model ran your work without
saying so would be the same thing wearing a different hat.

## Engines

Two are supported. Which ones you're offered depends on what's installed — `aichip doctor`
and `GET /api/engines` both answer by *running* each CLI.

| | Claude Code | OpenCode |
|---|---|---|
| Model ids | fixed catalog (`claude-opus-5`) | `provider/model`, from `opencode models` |
| Ask permission mid-run | yes | **no** — headless it rejects every prompt |
| Resume a session | yes | yes |
| Rate-limit signal | structured, so the queue backs off precisely | best-effort text match |
| Providers | your Claude login | whatever you've authenticated (`opencode providers list`) |

Because OpenCode cannot stop and ask, starting a **Reviewed** card on it is refused with a
`409` and a reason, at the click that caused it. Auto-edit works: aichip generates a
permission allow-list from the run's tools instead of answering prompts one at a time. That
refusal is deliberately *not* a silent downgrade — quietly turning Reviewed into Auto-edit
would be a privilege escalation performed on your behalf.

Tier defaults for a multi-provider engine are derived at boot from the models that install
can actually reach, rather than hard-coded: an `anthropic/…` default is wrong for someone
whose only provider is Google, and they'd discover it when their first task failed.

### Scheduled runs never park

A step that stops to ask holds its concurrency permit for the whole run, so a
scheduled workflow blocking on a permission prompt at 3am doesn't just go
unanswered — it eats one of `AICHIP_MAX_CONCURRENT` (default 2) until the server
restarts. Two of them and nothing dispatches at all.

So a **scheduled** run whose step resolves to Reviewed fails with a reason
naming the step, instead of parking. Manual runs still park: someone chose to
start them and is there to answer.

The common way to hit this isn't writing `permission_mode: reviewed` — it's
writing `full_auto` on a project that hasn't opted in, which is cut down to
Reviewed by the safety gate. The failure message says which of the two happened.

## Database

`aichip serve` manages its own Postgres by default. To use your own instead:

```bash
docker compose up -d
export DATABASE_URL=postgres://aichip:aichip@localhost:5433/aichip
cargo run -p aichip-cli -- serve
```

See `.env.example` for the other knobs.

## Running in Docker

aichip works by spawning *your* `claude` CLI under *your* login, so the interesting
question is how a container authenticates. On macOS the login lives in the **Keychain**
(there is no credentials file to mount), and a container has no keychain and no browser
to log in with. A container with your `~/.claude` mounted still reports
`Not logged in · Please run /login`.

The one way in is a long-lived token:

```bash
claude setup-token
```

Put it in `.env` as `CLAUDE_CODE_OAUTH_TOKEN`, set `AICHIP_PROJECTS_DIR` to the folder
holding your code, then:

```bash
docker compose --profile app up -d --build
```

That runs everything — dashboard, orchestrator, and the agents — in containers, reachable
at `http://localhost:4820`.

**Know what you're trading.** The token is a real credential sitting in a file, valid
until you revoke it, rather than a keychain entry scoped to your machine. aichip itself
still never reads, stores, or forwards it — the container inherits it from the environment
you set — but a token in `.env` is a broader exposure than the ordinary login, so keep
`.env` out of version control (it is gitignored) and revoke the token when you're done.

Two things the compose file handles that are easy to get wrong alone: your projects are
mounted at the **same absolute path** inside and out, because a git worktree records
absolute paths and a repo mounted elsewhere has a broken worktree link; and the container
runs as your UID, so files the agents write stay yours instead of root's.

**The recommended shape is still Postgres in a container and the server on your machine**
(`docker compose up -d`, above). You keep the ordinary keychain login, no token exists to
leak, and your paths are simply real. Containerize the whole thing when you want it on a
Linux box, running unattended, or away from your laptop — not because it's tidier.

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
- `crates/aichip-engines` — engine adapter trait, Claude Code and OpenCode adapters, mock engine
- `crates/aichip-core` — db, run orchestrator, worktree manager, queue, scheduler
- `crates/aichip-server` — axum REST + WebSocket + MCP permission proxy
- `crates/aichip-cli` — the `aichip` binary (`serve`, `doctor`)
- `web/` — React dashboard
