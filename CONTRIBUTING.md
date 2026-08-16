# Contributing to aichip

aichip orchestrates official coding-agent CLIs — Claude Code and OpenCode today — as child
processes on your own machine, under your own subscription login. It is process orchestration,
not API access, and that distinction is what most of the rules below are protecting.

The project is MIT licensed. Patches, bug reports and questions are welcome.

## Getting set up

You need:

- A stable Rust toolchain. CI builds on `stable`, so anything current works.
- `git` on `PATH`. aichip shells out to it for worktrees, diffs and merges.
- Node 22 and pnpm 10, if you are touching the dashboard. CI pins those two.
- On Debian or Ubuntu, `pkg-config` and `libssl-dev` for the Rust build.

You do **not** need a Postgres. `aichip serve` downloads, initialises and manages a private one
under `~/.aichip/pgdata` on first run. If you would rather point it at your own, `docker compose
up -d` and export `DATABASE_URL=postgres://aichip:aichip@localhost:5433/aichip`; the other knobs
are documented in `.env.example`.

Build the workspace and check your machine:

```bash
cargo build
cargo run -p aichip-cli -- doctor
```

`doctor` reports on git, `gh` if you have it, and every agent CLI it can find. It answers "is
this CLI logged in?" by *running* the binary, never by reading its config — see the invariants
below. An engine that is not installed is reported with a dot rather than a cross, because
aichip is usable with any one of them.

Then run the dashboard. The Rust server serves `web/dist`, so build the front end once before
starting it:

```bash
cd web && pnpm install && pnpm build
cd .. && cargo run -p aichip-cli -- serve      # http://127.0.0.1:4820
```

`serve` takes `--port` and `--headless` (the latter stops it opening a browser). Run it from the
repository root, since the `web/dist` default is relative to the working directory.

For front-end work you want the Vite dev server instead, with the Rust server running beside it:

```bash
cargo run -p aichip-cli -- serve --headless    # terminal one
cd web && pnpm dev                             # terminal two
```

Vite proxies `/api` and `/ws` through to `127.0.0.1:4820`, so the dev server gives you hot
reload against a real backend. If the dashboard loads but every request 404s, the server on
4820 is not running.

## Running the tests

```bash
cargo test                                             # the whole workspace
cargo test -p aichip-core                              # one crate
cargo test -p aichip-core backoff_escalates_and_caps   # one test by name

cd web && pnpm test                                    # vitest
cd web && pnpm test src/lib/workflowGraph.test.ts      # one file
```

**The test suite never talks to a model.** Rust tests that need an engine drive the mock engine
(`crates/aichip-engines/src/mock/`), which replays recorded stream-json fixtures with
configurable pacing. The web tests are pure vitest over the logic in `web/src/lib`. Nothing in
either suite needs an API key, a subscription, a database or a network call — so running the
tests costs nothing and cannot burn your rate limit. Run them freely.

CI runs four more things that are worth running locally before you push, because they fail the
build:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cd web && pnpm exec tsc -b
cd web && pnpm build
```

The `pnpm build` step is there because `web/dist` is what the server actually serves: a build
that fails is a broken release even when every test passes.

## Compliance invariants

These four rules are stated at the top of
[crates/aichip-engines/src/lib.rs](crates/aichip-engines/src/lib.rs) and enforced across the
codebase. They are the reason aichip can drive a paid CLI at all. **Code that violates one of
them will not be merged**, however good the feature attached to it is.

**1. Adapters spawn official binaries found on `PATH` and read their stdout. Nothing else.**
An adapter's whole job is to launch the vendor's own CLI (`claude -p --output-format
stream-json`, `opencode run --format json`) and normalise what it prints into `AichipEvent`.
There is no HTTP control API in the loop and no reimplementation of an engine's protocol.

**2. Never read, store, extract or forward credentials.** Do not touch `~/.claude`, and do not
touch any other engine's config or credential files. This is why `detect()` and `aichip doctor`
establish "installed and logged in" by running the binary. Where a CLI can name its providers,
aichip surfaces the provider name and the auth *type* — "oauth", "api" — and never the secret.

**3. Never set authentication environment variables on a spawned process.** The single source
of truth for "does this name look like an auth secret" is
[crates/aichip-shared/src/env_guard.rs](crates/aichip-shared/src/env_guard.rs): use
`is_auth_env` and `auth_env_refusal`, never a hand-rolled prefix list. The module exists because
two hand-maintained Anthropic-only lists were already in the tree, and nothing in
`["ANTHROPIC_", "CLAUDE_CODE_OAUTH"]` stops `OPENAI_API_KEY`. The check is deliberately broad —
a false positive costs someone one confusing refusal, a false negative hands a credential to a
subprocess. Separately, `AICHIP_OWN_SECRETS` are stripped from every child, because a spawned
CLI inherits the server's whole environment and would otherwise be handed aichip's own storage
credentials having passed no check at all.

**4. Never proxy, intercept or replay engine network traffic.** Whatever the engine says to its
provider is between the two of them.

If you are adding a third engine adapter, those four rules are most of the specification. The
other half is `Capabilities`, which has deliberately no `Default` impl: a new adapter has to
state its own answers, because inheriting "yes, I can do everything" by omission is how a
descriptor like that rots into a lie. Gate behaviour on a capability, never on `if engine ==
"..."`.

## Commit conventions

Commits and pull request bodies in this repository carry **no AI attribution of any kind**:

- No `Co-Authored-By` trailer naming a model or tool.
- No "Generated with ..." footer, and no link back to a vendor's site.
- No "AI-assisted", "written by an agent" or equivalent note in the commit body, the PR
  description, or a code comment.

Write the message the way an author writes one: what changed and why, in the imperative. The
existing history is the model to follow.

Product-level mentions of Claude Code and OpenCode are a different thing entirely and stay —
they are the engines aichip drives, so `ClaudeEngine`, model ids, README text and UI labels are
ordinary code.

## Where things go

**Rust tests live next to the code they test**, in an inline `#[cfg(test)] mod tests` at the
bottom of the file. There is no `tests/` directory in any crate and adding one would be the odd
one out. Name the test after the behaviour it pins rather than the function it calls — the
existing ones read as sentences (`an_engine_that_cannot_ask_refuses_reviewed_rather_than_downgrading`),
which is what makes a failure legible in CI output.

**Pure logic on the web side belongs in `web/src/lib/*.ts`**, not in a component. That is the
line the vitest suite is drawn along: anything in `lib` can be tested without a DOM, and
anything in a `.tsx` component effectively cannot be. If you find yourself wanting a test for
something inside a component, the answer is usually to move the calculation into `lib` and let
the component render its result.

**Migrations go in `crates/aichip-core/migrations/`**, numbered in sequence
(`0069_thing.sql`). They are applied at boot. Additive changes are cheap; anything that drops
or rewrites data deserves a note in the file saying why.

Some behaviour is deliberately specified once and shared by both test suites — the expression
language, for instance, exists in `crates/aichip-core/src/apps/expr.rs` and
`web/src/lib/expr.ts`, and `crates/aichip-core/src/apps/expr_cases.json` is the specification
both read. Add a case to the JSON, not to one side.

## Gotchas that will bite you once

**A new migration may not be picked up.** sqlx embeds `crates/aichip-core/migrations/` into the
binary at compile time via `sqlx::migrate!("./migrations")`, and adding a file does not reliably
retrigger a rebuild. The symptom is a new column coming back as `ColumnNotFound` even though the
SQL is obviously right there. The fix:

```bash
touch crates/aichip-core/src/db.rs
cargo build
```

**A green `cargo test` is not a running server.** The test run does not replace the binary you
launched, and a server already running keeps the code it started with. To see a change in the
dashboard: `cargo build`, stop `aichip serve`, start it again. Front-end changes are different —
`pnpm dev` hot-reloads them, but a change you only made in `web/dist` via `pnpm build` still
needs a page refresh.

**There is no dark mode, and colours come from tokens.** The palette is defined once in the
`@theme` block at the top of `web/src/index.css` (`--color-surface`, `--color-ink`,
`--color-line`, the tier accents, the tint pairs). There is not a single `dark:` variant in the
tree, and a hex literal dropped into a component is a bug even when it looks right on your
screen — it cannot follow the tokens when they move. Use a token, or add one.

**The icon name union is closed.** `web/src/components/ui/Icon.tsx` hand-draws its glyphs rather
than pulling in an icon package, so `IconName` is an explicit union and `P` is a
`Record<IconName, ...>` over it. Passing a name that is not in the union is a type error, not a
missing glyph at runtime. To add one, extend both the union and the record, keeping the shared
grid (24), weight (1.75) and round caps — that consistency is the only thing making a row of
them look like a set. Draw in `currentColor` so the glyph inherits its surroundings.

## Proposing a change

For anything large — a new engine adapter, a schema change, a feature that adds a page, or
anything touching the compliance surface — **open an issue first**. Describe what you want to
do and why. It is much cheaper to disagree about an approach in an issue than in a review of
four hundred lines that already work.

For small fixes, go straight to a pull request. A typo, a wrong error message, a missing guard,
a test that pins something that was previously only true by accident — none of those need a
preamble.

Either way, a good pull request:

- does one thing, and says in the description what that thing is and why it is wanted;
- comes with tests where the behaviour is testable without a model, which the mock engine makes
  true of nearly everything;
- passes `cargo fmt`, `cargo clippy -D warnings`, both test suites and `pnpm build`;
- explains its reasoning in comments where the code is doing something non-obvious. The
  existing comments in this codebase state *why*, not *what*, and a patch that follows that
  habit is easier to accept.
