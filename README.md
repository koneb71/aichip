# aichip

**A local-first multi-agent workflow platform for coding agents — no API keys.**

aichip is a dashboard for running the coding-agent CLIs you already have installed.
It spawns [Claude Code](https://code.claude.com), [OpenCode](https://opencode.ai) and
[Codex](https://developers.openai.com/codex/cli) as child processes on your own machine,
under your own subscription login, and gives them a board, a queue, git worktrees, a diff
to review, and a record of what everything cost. Models served locally by
[Ollama](https://ollama.com) or [LM Studio](https://lmstudio.ai) are offered the same way,
and cost nothing to run.

It is for people who already work with one of these CLIs in a terminal and want more than
one thing happening at a time — several cards in flight, each in its own worktree, with a
place to see what they did before any of it reaches your working copy.

## What makes it different

- **It runs your CLI, not an API.** No API key goes anywhere near it, because none is
  needed: the binary on your `PATH` is already logged in, and aichip just starts it.
- **Everything is local.** Postgres runs under `~/.aichip`, the code index and document
  embeddings are computed on your machine, and nothing is sent anywhere aichip controls.
- **The review surface is git.** A board task runs in an isolated worktree, so the thing
  you approve is an ordinary diff on an ordinary branch, and the thing you reject costs
  you a deleted branch rather than an undo.
- **Refusals are up front.** An engine that cannot pause to ask for permission is refused
  at the click, not silently downgraded; a schema change that drops a column waits for
  you; a plan-first run has the mutating tools *denied*, not merely left off a list.

## Contents

- [How it stays within the terms of service](#how-it-stays-within-the-terms-of-service)
- [Status](#status)
- [Requirements](#requirements)
- [Quick start](#quick-start)
- [The board](#the-board)
- [Adding a folder](#adding-a-folder)
- [Attachments and file references](#attachments-and-file-references)
- [The Map tab](#the-map-tab)
- [Chat](#chat)
- [Research](#research)
- [Document spaces](#document-spaces)
- [The Brain and skills](#the-brain-and-skills)
- [Routines](#routines)
- [A project manager](#a-project-manager)
- [Workflows](#workflows)
- [Organizations](#organizations)
- [Apps](#apps)
- [Knowledge base](#knowledge-base)
- [What it costs, and spending less](#what-it-costs-and-spending-less)
- [Engines](#engines)
- [Database](#database)
- [Running in Docker](#running-in-docker)
- [Development](#development)
- [Workspace layout](#workspace-layout)
- [Contributing](#contributing)
- [Licence](#licence)

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

These four invariants are stated at the top of
[`crates/aichip-engines/src/lib.rs`](crates/aichip-engines/src/lib.rs) and are contribution
rules. PRs that violate them will not be merged.

## Status

Early development, and version 0.1. The board, agents, teams, chat, research, workflows,
routines, apps, the knowledge base and the code map all work end to end. Interfaces still
move between commits, and there is no migration story for anything but the database.

## Requirements

- **macOS or Linux.** Windows is not supported; nothing has been tested there.
- **A Rust toolchain** (stable, 2021 edition) to build the workspace.
- **Node and pnpm** to build the dashboard. The server serves `web/dist`, so a source
  checkout needs `pnpm build` once before `serve` has a UI to hand out.
- **git** on `PATH`. It is not optional: worktrees are how a task stays reviewable.
- **At least one agent CLI on `PATH`** — `claude`, `opencode` or `codex` — already logged
  in. `aichip doctor` tells you which ones it found, and where to get the ones it didn't.

Optional:

- **`gh`**, for cloning from GitHub, importing issues as cards, and opening pull requests.
  Everything else works without it, and `doctor` reports a missing `gh` as a note.
- **Docker**, only for branch previews, for running Postgres yourself, or for the MinIO
  bucket that holds files pasted into knowledge-base pages. Nothing else needs it.

The first document you index and the first project you map download a small embedding
model (about 35 MB) into `~/.aichip/models`. That is an artifact download, the same class
as a cargo dependency; no content leaves the machine in either direction.

## Quick start

```bash
cargo run -p aichip-cli -- doctor   # checks git + every agent CLI it can find
cd web && pnpm install && pnpm build && cd ..
cargo run -p aichip-cli -- serve    # starts the dashboard on http://127.0.0.1:4820
```

The first `serve` downloads and initializes a private Postgres under `~/.aichip/pgdata`,
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

## The Map tab

What a project is made of, read out of the code rather than written down. The index is
built on demand — when the project page opens, when a card's work lands, when `HEAD`
moves — and a sha256 hash-diff makes the repeat passes cheap: an unchanged file costs one
read and one hash.

Three ways in, answering different questions:

- **Search by meaning.** "Where does the thing that rate-limits live" is a question
  `grep` cannot answer, because grep needs the word and not knowing the word is the whole
  problem. Ranking is cosine similarity over embeddings computed locally with ONNX
  inference; no model API is called and no vector extension is required. Agents get the
  same search as an MCP tool, so a run does not start by reading the directory tree.
- **A graph you can open up.** Symbols and imports come from tree-sitter — a real parser,
  because a regex cannot tell a definition from the same words inside a comment, and the
  map's promise is that a name it shows exists at the line it shows. Rust, TypeScript,
  TSX and Python have their insides drawn; other files are still listed and still
  searchable. An import that cannot be resolved against the project's real file list
  becomes nothing rather than a guess: a wrong edge sends you to change a file that has
  nothing to do with yours.
- **A list**, which answers the same as the graph without needing a mouse.

Node size is PageRank over the import edges — twenty-five lines of power iteration rather
than a graph library. It sizes dots and breaks ties; it deliberately does not order
search results, because "most depended upon" reliably names the infrastructure everybody
already knows about and never the file you should be editing.

Everything here is derived and never authored, so each rendering names the branch and
commit it was read at.

## Chat

Every project has an assistant beside its board, and there is a project-less General chat
for everything that is not about one repository. It reads the code and the board, and it
can file and start cards.

### Plan mode

That last part is where a misunderstanding turns into money: the assistant creates a card,
assigns an agent, starts it, and the first sign it had the wrong idea is a run that has
already spent. **Plan mode** takes the four acting tools away for a turn — create, start,
move and cancel — asks for a plan instead, and gives you a button.

The plan turn *finishes* like any other reply rather than parking, because a parked run
would block the next message in the conversation it is meant to be part of. So you can
argue with a plan in the next sentence. Approve carries it out; **Edit first** opens the
plan as text and the edited version is what is authoritative; there is no Reject button
because closing the plan and typing something else is already the answer.

### It asks instead of guessing

In plan mode the assistant is told to ask before writing a plan on a wrong assumption.
The question arrives as a card with **options**, not as prose: a closed set is unambiguous
in both directions, and it forces the assistant to have thought of the alternatives rather
than merely noticing it was unsure. A single question with a single answer is answered by
clicking the option — a Send button after that is a step with no decision in it. Anything
else (several questions, or a multi-select) keeps the button, because nothing else can
know when you have finished choosing. At most four questions at once, with two to four
options each; past that it is interviewing rather than clarifying. There is always a way
out that is not one of the options, because the composer is right below and a question you
can only answer its own way is a form. Answering keeps you in plan mode, with the answer
in hand.

## Research

Ask a question about a project and get back a cited markdown report. A research run is
read-only over your **real checkout** — no worktree, no branch — with `Read`, `Grep`,
`Glob` and the CLI's own `WebSearch` and `WebFetch`, which is the half no other run type
gets. The five mutating tools plus `Task` are denied outright: research runs at the strong
tier, and a subagent fan-out would multiply that spend invisibly while making the live
transcript unreadable.

The report lands on the Research page and can be filed into the knowledge base with one
click. Filing is idempotent — the second click returns the article the first one made.

## Document spaces

A project does not have to be a repository. A **space** is a folder of documents: drop in
`md txt csv json log pdf docx pptx xlsx xlsm xls ods` and they are chunked, embedded and
searchable. Legacy `.doc` and `.ppt` are refused at upload rather than accepted and left
permanently unreadable.

Chatting in a space retrieves the passages that answer the question and folds them into
the prompt, cited by file and position. Retrieval is the same local pipeline the Map tab
uses: chunks in Postgres, embeddings from ONNX inference on your machine, ranking by
brute-force cosine in Rust. A space is thousands of chunks, and brute force ranks that in
milliseconds — no vector extension required.

Retrieved passages are fenced and labelled as reference material before they reach a run,
for the same reason knowledge-base pages are: the text arrived from a file somebody else
wrote, and a run holding Edit and Bash should not treat it as instructions.

## The Brain and skills

Two ways to stop retyping the same context into every card.

**The Brain** is a project's standing context — *"the API lives in `/backend`"*, *"we do
not add dependencies without asking"*. It reaches every run in that project without
anybody remembering to attach it, which is the point: a thing you must remember every time
is a thing you use no times.

**A skill** is how one particular job is done here — the release checklist, the way
migrations get written, what a bug report has to contain. An agent is *who* does the work;
a skill is *how* this job goes. It applies when you name it — `@its-name` in chat, or
picked on a card — and never because something matched a description. That is a deliberate
refusal of the more magical design: a skill that only applies when you name it cannot
steer a request that never mentioned it, and when one misbehaves the cause is the thing
you just typed.

Both are user-editable text pasted into a run holding Edit, Write and Bash, so both get the
same treatment: framed as background rather than orders, unable to close their own fence,
and capped so neither can bury the actual task. Text that looks like a credential is
refused on save.

### Installing a skill from a registry

aichip can install Agent Skills into a project:

```
npx skills add owner/repo
```

is run in the project, and what lands is a real skill — `.agents/skills/<name>/` with its
`SKILL.md` and whatever it bundles, symlinked into `.claude/skills/` so Claude Code reads
it natively. That folder is the copy with full fidelity: a skill shipping
`resources/deploy.sh` still has its script.

Each `SKILL.md` is then mirrored into an aichip skill row, so the same skill can be
`@name`d in a chat, bound to a card, and carried to an engine that has never heard of the
format. **The folder is what wins.** The row is re-derived from disk on every install and
every sync, so an edit made to the mirror is overwritten the next time either happens —
copy it into a skill of your own if you want to change it.

Two things this deliberately does not do. It never installs globally: `-g` writes to
`~/.claude/skills`, and not touching an engine's own directory is the second compliance
invariant. And it reads `skills-lock.json` for what happened rather than parsing the
installer's stdout, which is a spinner and a box-drawn table full of escape codes.

The result is committed, because a worktree is branched from `HEAD` and an uncommitted
skill never reaches a card run at all. Whatever arrived that is not markdown — the scripts
and data an agent may execute — is listed back to you, because the installer's own parting
advice is to review skills before use and a list of what landed is the only version of
that advice you can act on.

## Routines

A prompt that runs on a schedule. Four kinds, each landing where that work naturally
lives:

- **Chat** — a turn in the routine's own standing thread, so the replies collect in one
  place and this morning's answer knows what yesterday's said.
- **Research** — a fresh cited report each time.
- **Task** — a card created and started on a project's board.
- **Watch** — check a page on a schedule and report what changed.

A routine never executes anything itself. Firing only *enqueues* through the same doors a
person uses, so concurrency limits, backoff, permission prompts and spend accounting
behave exactly as they do for manual work. The history row is written *before* the work,
which is why a firing that produced nothing still shows up saying why — "didn't run: the
assistant was still working" — instead of the list quietly thinning out.

The "next run" time on the page comes from the same cron parser that will fire it, so it
is not a second implementation that can disagree.

## A project manager

A project can be given a **manager**: an agent that reviews the board on a cron, with
nobody watching.

A pass is a turn in the project's standing manager thread, so the session resumes and this
morning's pass can say what moved since yesterday's — the difference between a manager and
a series of strangers each meeting the board for the first time. It lists the cards, reads
diffs and statuses to find out what actually happened rather than assuming it went well,
and finishes with a few lines: what changed, what it did, what it left alone, what it
wants you to look at.

What keeps it honest is a **cap on how many cards one pass may start** — two by default,
ten at the most, and zero is a legitimate setting for a repository you are not ready to
let it touch. The cap is enforced by counting rows in the actions log inside the tool
handler, so the model cannot talk its way past it; it is also stated in the prompt,
because an agent that discovers its budget by being refused wastes the refusal and tends
to retry. Cards it creates without starting cost nothing and are the right answer whenever
it is unsure.

Two more rails worth knowing:

- **It cannot start a card that came from outside aichip.** An imported issue was written
  by somebody who is not the owner of this machine, and a person belongs between that text
  and an agent that can write files.
- **Board text is material, not instructions.** A card telling the manager to ignore its
  limits is content somebody typed; the manager is told to report it rather than obey it.

Every acting tool call is recorded, and the project's manager panel shows what each pass
did — which is the question a manager has to answer that a plain routine does not.

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
    model: easy                    # easy | medium | complex, mapped per engine
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

## Apps

Everything else here changes code you already have, and lands as a diff you
review. An app is the other thing: something you ask for, install, switch on,
and **use**.

An app is a manifest — one YAML file. Models in it become **real Postgres
tables**; views become screens aichip's own dashboard draws. Nothing you get
handed executes:

```yaml
name: Expenses
icon: "▤"
runtime: module

models:
  expense:
    fields:
      description: { type: text, required: true }
      amount:      { type: decimal }
      qty:         { type: int, default: 1 }
      total:       { type: decimal, compute: "amount * qty" }
      spent_on:    { type: date, default: "today()" }
      category:    { type: text }
    indexes: [spent_on]

views:
  list:  { columns: [spent_on, description, category, total], sort: "-spent_on" }
  chart: { shape: bar, group_by: category, measure: "sum(total)" }

menu:
  - { label: Expenses, view: list }
  - { label: By category, view: chart }
```

Describe what you want and an agent writes that file. It comes back **in the
editor, not installed** — being able to read the thing before it is real is the
entire reason an app is a declaration rather than code, and installing it for
you would spend that and give nothing back.

Field types are a closed set (`text int decimal bool date datetime json` and
`ref:<model>`), and names are lower-case letters, digits and underscores. That
narrowness is load bearing: these identifiers are interpolated into DDL, and the
defence is the charset rather than the quoting. Unknown keys are refused rather
than ignored — an agent that writes `colums:` has written a view with no
columns, and silently rendering an empty table is worse than saying which key.

### Changing one, and undoing it

**Change this app** hands it back to an agent: say what should be different, and
it works in a worktree of the app's own folder like any other card. For a module
it rewrites the manifest; for a container app it writes real source.

That change **lands on its own** when the card finishes — there is no review
step, because the diff *is* the app, and asking you to read a patch before you
can see whether the chart came out right turns a gallery back into a task board.
The repository being merged into is the one aichip created for that app, never
your code.

What makes that bargain honest is that the undo is real. Every change records
where the app stood before it, and **Undo** on the newest one puts the folder
back exactly there. Only the newest, deliberately: an older change's starting
point knows nothing about the ones after it, and offering that button would
silently throw them away.

Landing files is not landing schema. The manifest is read back off disk and goes
through the same gate below, so a change that drops a column still waits for you
even though the file it came in has already merged. If the merge conflicts,
nothing lands — the card stays in review with its diff, which is the flow every
other task already uses.

### Sharing one

An app is a folder, so sharing is a file. **Share** exports the app with empty
tables — what you send someone. **Export with data** carries the rows too — what
you move to another machine. Import regenerates the DDL from the manifest and
never runs the bundled `schema.sql`, which is there to be read.

For a team, commit it. Anything under `.aichip/apps/` in a repository you have
added shows up on the gallery page with an **Install** next to it, and a manifest
being plain YAML is what makes it reviewable in a pull request. Syncing an app
you already have replaces its manifest and keeps its rows.

### Your tables are yours

Change the manifest and the tables follow, but not unconditionally. New tables,
new columns and new indexes apply themselves, because asking about them would be
a dialog that always gets the same answer — and a dialog that always gets the
same answer trains people to stop reading it.

Anything that **destroys** something waits, whole. A dropped column, a dropped
table, a changed type: you get the literal SQL and a sentence saying what it
costs, and nothing has run until you say so. What you approve is byte for byte
what executes, because deriving it again at that point would mean running
statements nobody saw.

The comparison is against `information_schema`, not against a registry of what
aichip thinks the schema is. A registry drifts the first time anything touches a
table outside the code maintaining it, and then every diff is against a fiction.

### Switching off is not deleting

**Deactivate** takes an app out of the sidebar and keeps every row.
**Uninstall** is the only verb that drops a schema, and it asks first. If
deactivating ever risked data the switch would stop being used, which is the
whole point of having one.

### What an app can and cannot reach

Its own tables, always — they exist because it declared them, hold only what it
put there, and are dropped with it. Anything of *yours* is a scope the manifest
requests and you grant, and putting a card on your board is a different grant
from starting an agent run: filing a card is cheap, and spending money is not
the same decision.

An app never gets a database connection, and never writes SQL. It says
`amount:gt:10`; the grammar is closed, identifiers are looked up among declared
fields, and every value is a bound parameter. A note of `'; DROP TABLE entry; --`
is stored, returned and matched as exactly those characters.

Decimals stay text the whole way — string in, `numeric` column, string out — so
a ledger does not lose cents to a double on its way to a browser. A number with
more digits than a JSON number can carry is refused rather than quietly rounded,
and the message says to send it as a string.

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

Which ones you're offered depends on what's installed — `aichip doctor` and
`GET /api/engines` both answer by *running* each CLI, never by reading its config.

| | Claude Code | OpenCode | Codex |
|---|---|---|---|
| Model ids | fixed catalog (`claude-opus-5`) | `provider/model`, from `opencode models` | OpenAI ids (`gpt-5-codex`) |
| Ask permission mid-run | yes | **no** — headless it rejects every prompt | **no** |
| Resume a session | yes | yes | yes |
| Rate-limit signal | structured, so the queue backs off precisely | best-effort text match | best-effort text match |
| Providers | your Claude login | whatever you've authenticated (`opencode providers list`) | your OpenAI login |
| Tier defaults | fixed catalog | derived from `opencode models` | derived from `codex doctor` |

Codex is driven through `codex exec --json`, and everything aichip needs to say about a
run — the sandbox, the approval stance, the persona, aichip's own MCP endpoint — is passed
as `-c key=value` overrides rather than written to `~/.codex/config.toml`, which the second
compliance invariant forbids touching. Your own config still merges in underneath.

Because OpenCode cannot stop and ask, starting a **Reviewed** card on it is refused with a
`409` and a reason, at the click that caused it. Auto-edit works: aichip generates a
permission allow-list from the run's tools instead of answering prompts one at a time. That
refusal is deliberately *not* a silent downgrade — quietly turning Reviewed into Auto-edit
would be a privilege escalation performed on your behalf.

Tier defaults for a multi-provider engine are derived at boot from the models that install
can actually reach, rather than hard-coded: an `anthropic/…` default is wrong for someone
whose only provider is Google, and they'd discover it when their first task failed.

### Local models: Ollama and LM Studio

Both appear in the engine picker alongside the others, and a run on either costs nothing
and leaves the machine at no point.

Under the hood they are not separate agents — they can't be, because an inference server
serves a model and holds no tools. Picking **Ollama** or **LM Studio** runs the `opencode`
binary with that runtime declared as its provider and the model resolved from what the
runtime actually reports (`ollama list`, `lms ls --json`). So both need OpenCode installed
as well; `doctor` says so when it's the missing piece, and distinguishes *not installed*
from *installed, but its server isn't running*.

Two things to know before you pick one:

- **The model has to support tool calling.** A coding agent reads and edits files through
  tools, so a chat-only or pure-reasoning model can't do the job — Ollama's `deepseek-r1`,
  for instance, answers `does not support tools` and the run fails. aichip does not filter
  these out, because older Ollama can't say which models are which and silently hiding a
  model you can see in your own library is worse than a clear error.
- **The context window has to fit the prompt.** aichip's chat prompt is around 13k tokens
  before your message; a model loaded with an 8k window will refuse it. Raise it in
  LM Studio, or `num_ctx` in Ollama.

Discovery is separate from all of this: Settings → *Local model runtimes* asks both servers
over HTTP so the model fields can offer what you have, and works whether or not OpenCode is
installed.

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
cd web && pnpm test    # vitest — canvas ↔ YAML round-trip, diff, mention, kb tree
```

The mock engine replays recorded stream-json fixtures with configurable pacing and is the
backbone of the Rust suite, so a full `cargo test` spends nothing and cannot be rate
limited. Rust tests live inline in `#[cfg(test)] mod tests` next to the code they cover.

Adding a migration under `crates/aichip-core/migrations/` does not always
retrigger a rebuild, because sqlx embeds them at compile time. If a new column
comes back as `ColumnNotFound`, `touch crates/aichip-core/src/db.rs` and rebuild.

## Workspace layout

- `crates/aichip-shared` — event types, model tiers, workflow YAML, the auth-env guard
- `crates/aichip-engines` — engine adapter trait; Claude Code, OpenCode, Codex and local (Ollama / LM Studio) adapters, mock engine
- `crates/aichip-core` — db, run orchestrator, worktree manager, queue, scheduler, apps, RAG, code map
- `crates/aichip-server` — axum REST + WebSocket + MCP permission proxy + preview proxy
- `crates/aichip-cli` — the `aichip` binary (`serve`, `doctor`)
- `web/` — React 18 + Vite + Tailwind 4 dashboard

## Contributing

Issues and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) — it
covers the build, the test story, and the four compliance invariants that decide whether a
change to the engine layer can be merged at all.

## Licence

MIT. See [LICENSE](LICENSE).
