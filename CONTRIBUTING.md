# Contributing to Eren

Eren orchestrates official coding-agent CLIs — Claude Code, OpenCode, Codex, Gemini CLI, Cursor
CLI, Qwen Code and Amp, plus Ollama and LM Studio models driven through OpenCode — as child
processes on your own machine, under your own subscription login. It is process orchestration,
not API access, and that distinction is what most of the rules below are protecting.

The project is MIT licensed. Patches, bug reports and questions are welcome.

## Getting set up

You need:

- A stable Rust toolchain. CI builds on `stable`, so anything current works.
- `git` on `PATH`. Eren shells out to it for worktrees, diffs and merges.
- Node 22 and pnpm 10, if you are touching the dashboard. CI pins those two.
- On Debian or Ubuntu, `pkg-config` and `libssl-dev` for the Rust build.

`./scripts/setup.sh` installs all of that on macOS (with Homebrew) or Linux (apt, dnf, pacman
or zypper), skipping whatever is already there; `--dry-run` shows what it would do first.

You do **not** need a Postgres. `eren serve` downloads, initialises and manages a private one
under `~/.eren/pgdata` on first run. If you would rather point it at your own, `docker compose
up -d` and, for Eren, export `DATABASE_URL=postgres://aichip:aichip@localhost:5433/aichip` —
compose keeps its legacy role and database names because they name an existing volume (the
comment at the top of `docker-compose.yml` explains). The other knobs are documented in
`.env.example`.

Build the workspace and check your machine:

```bash
cargo build
cargo run -p eren-cli -- doctor
```

`doctor` reports on git, `gh` if you have it, and every agent CLI it can find. It answers "is
this CLI logged in?" by *running* the binary, never by reading its config — see the invariants
below. An engine that is not installed is reported with a dot rather than a cross, with where to
get it, because Eren is usable with any one of them. For an installed engine it also says what
it cannot do (ask permission mid-run, signal a rate limit, carry Eren's tools).

Then run the dashboard. The Rust server serves `web/dist`, so build the front end once before
starting it:

```bash
cd web && pnpm install && pnpm build
cd .. && cargo run -p eren-cli -- serve      # http://127.0.0.1:4820
```

`serve` takes `--port` and `--headless` (the latter stops it opening a browser). Run it from the
repository root, since the `web/dist` default is relative to the working directory
(`EREN_WEB_DIST` overrides it).

For front-end work you want the Vite dev server instead, with the Rust server running beside it:

```bash
cargo run -p eren-cli -- serve --headless    # terminal one
cd web && pnpm dev                           # terminal two
```

Vite proxies `/api` and `/ws` through to `127.0.0.1:4820`, so the dev server gives you hot
reload against a real backend. If the dashboard loads but every request 404s, the server on
4820 is not running.

## Running the tests

```bash
cargo test                                           # the whole workspace
cargo test -p eren-core                              # one crate
cargo test -p eren-core backoff_escalates_and_caps   # one test by name

cd web && pnpm test                                  # vitest
cd web && pnpm test src/lib/workflowGraph.test.ts    # one file
```

**The test suite never talks to a model.** Rust tests that need an engine drive the mock engine
(`crates/eren-engines/src/mock/`), which replays recorded stream-json fixtures with
configurable pacing; the real adapters are tested against fixtures and stand-in binaries. The
web tests are pure vitest over the logic in `web/src`. Nothing in either suite needs an API key,
a subscription or a network call — so running the tests costs nothing and cannot burn your rate
limit. Run them freely.

Tests that exercise SQL start with `testdb::fresh()` and skip when `DATABASE_URL` is unset; each
makes and drops a database of its own on that server. To run them locally against compose:

```bash
DATABASE_URL=postgres://aichip:aichip@localhost:5433/aichip cargo test   # compose's legacy names; Eren's tests make their own databases
```

CI runs them against a Postgres service.

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) also runs these, and all but clippy
fail the build:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets     # advisory in CI for now; keep it clean anyway
cd web && pnpm exec tsc -b
cd web && pnpm build
```

The `pnpm build` step is there because `web/dist` is what the server actually serves: a build
that fails is a broken release even when every test passes.

Several rules are held by tests that read the source rather than by review, and they fail
`cargo test` / `pnpm test` like any other test: spawning a process other than through
`env_guard::command`, the project's old name outside the compatibility files, a second writer of
the audit log, `project_checks`, the review policy or a config revision, a run inserted without
asking whether its agent may run, an agent tool whose name could merge, start or decide, a doc
that misses an engine, a capability, a setting or a migration, and a screen that uses a raw
colour. When one fails, the fix is in the code, not the scan.

## Compliance invariants

These four rules are stated at the top of
[crates/eren-engines/src/lib.rs](crates/eren-engines/src/lib.rs) and enforced across the
codebase. They are the reason Eren can drive a paid CLI at all. **Code that violates one of
them will not be merged**, however good the feature attached to it is.

**1. Adapters spawn official binaries found on `PATH` and read their stdout. Nothing else.**
An adapter's whole job is to launch the vendor's own CLI (`claude -p --output-format
stream-json`, `opencode run --format json`, `codex exec`, …) and normalise what it prints into
`ErenEvent`. There is no HTTP control API in the loop and no reimplementation of an engine's
protocol. Ollama and LM Studio are no exception: a local runtime holds no tools, so Eren drives
the OpenCode binary pointed at it.

**2. Never read, store, extract or forward credentials.** Do not touch `~/.claude`, and do not
touch any other engine's config or credential files. This is why `detect()` and `eren doctor`
establish "installed and logged in" by running the binary. Where a CLI can name its providers,
Eren surfaces the provider name and the auth *type* — "oauth", "api" — and never the secret.

**3. Never set authentication environment variables on a spawned process.** The single source
of truth for "does this name look like an auth secret" is
[crates/eren-shared/src/env_guard.rs](crates/eren-shared/src/env_guard.rs): use
`is_auth_env` and `auth_env_refusal`, never a hand-rolled prefix list. The module exists because
two hand-maintained Anthropic-only lists were already in the tree, and nothing in
`["ANTHROPIC_", "CLAUDE_CODE_OAUTH"]` stops `OPENAI_API_KEY`. The check is deliberately broad —
a false positive costs someone one confusing refusal, a false negative hands a credential to a
subprocess. Separately, Eren's own secrets (`env_guard::OWN_SECRETS`, under every name
`own_secrets()` lists) are stripped from every child, because a spawned CLI inherits the
server's whole environment and would otherwise be handed Eren's own storage credentials having
passed no check at all. That is why every process starts through `env_guard::command` and never
`Command::new` — `clippy.toml` says so in an editor, and a test in `env_guard.rs` fails the build.

**4. Never proxy, intercept or replay engine network traffic.** Whatever the engine says to its
provider is between the two of them.

## Adding an engine

Open an issue first. Then follow how Gemini CLI, Cursor CLI, Qwen Code and Amp were added
(`git show a94f3a0 --stat` lists every file that took):

1. **A module** `crates/eren-engines/src/<id>/mod.rs`, declared in
   `crates/eren-engines/src/lib.rs`, implementing `Engine`: a stable `id()`, a `label()`,
   `detect()` (by running the binary — `--version`, a status subcommand — never by reading its
   files), `start()`, and `interactive_resume_argv()` (`None` unless someone has checked the
   command against the real binary).
2. **Its own `Capabilities`.** There is deliberately no `Default` impl: answer every field, with
   a comment saying how you know. When unsure, say `false` — inheriting "yes, I can do
   everything" by omission is how a descriptor like that rots into a lie. Gate behaviour on a
   capability, never on `if engine == "..."`. `vet` and the orchestrator then refuse what the
   engine cannot honour at the click (a `409`) rather than quietly widening it, and Full Auto
   steps down only to a mode the engine has.
3. **Parse with `pump::LineParser`** (`crates/eren-engines/src/pump.rs`): implement `line()`
   (never fails; an unknown line is ignored) and `finish()` (the terminal event from the exit
   status and stderr tail when the stream gave none), and start the process with `pump::spawn`.
   A Claude-compatible stream can reuse `claude::compat::ClaudeCompat`, as Qwen and Amp do.
4. **Spawn only via `env_guard::command`**, and never set an auth variable on the child. Apply
   `RunSpec.denied_tools` last, refuse any `extra_env` key `is_auth_env` flags, and if the CLI has
   an environment namespace of its own (`CURSOR_`, `QWEN_`, `AMP_`, …), add it to
   `VENDOR_PREFIXES` in `env_guard.rs` with a test. Never write a config file into a worktree or
   the user's checkout — MCP wiring goes in a flag, an environment variable, or a file in Eren's
   own scratch directory, or the engine declares `mcp_tools: false`. An optional binary override
   is read with `brand::var("<ID>_BIN")`, never `std::env::var`.
5. **Fixtures** in `crates/eren-engines/src/<id>/fixtures/`: a recorded run if you have the CLI,
   otherwise synthetic ones built from its source or docs, with a `fixtures/README.md` that says
   they are **synthetic**, where they came from, and the command to record real ones with.
   Parser tests read them with `include_str!`.
6. **A stand-in binary test**: `crate::replaying(dir, fixture)` / `crate::stand_in(dir, body)`
   write a shell script that records its argv and prints the fixture, so the adapter runs end to
   end — argv, spawn, pump, parser — without the real CLI (`a_stand_in_binary_runs_end_to_end`
   in each of the four newer adapters). Unit-test the argv for each permission mode too.
7. **Register it** in `real_engines()` in `crates/eren-cli/src/main.rs` (one list for `serve`
   and `doctor`) and give `install_hint()` a line for it.
8. **Model tiers** in `crates/eren-shared/src/model_tier.rs`: a default in
   `EngineTierMapping::defaults_for` (empty when the ids are the person's provider's, as for
   Qwen) and a rule in `is_known_model_for`, with a test.
9. **The docs**: the engine id in backticks in README.md and CLAUDE.md, its capabilities and
   quirks in CLAUDE.md, any new `EREN_*` in `.env.example` and README.md, and SECURITY.md if it
   touches the compliance surface. `crates/eren-cli/src/docs_tests.rs` fails until the first
   and third are done.

## Web changes

The dashboard has a design system, and two tests hold it:

- **Tokens, never raw colours.** Every colour is a token in `web/src/index.css` — the `@theme`
  block for light, redefined under `:root[data-theme="dark"]` for dark (`bg`, `panel`, `fg`,
  `fg-muted`, `border`, `accent`, `success`/`warning`/`danger`/`info` with their `-fg` and
  `-subtle` pairs, the tier and tint accents). No Tailwind palette classes (`bg-gray-50`), no hex
  literals, no `bg-white` or `text-black`, which are right in one theme only. If no token fits,
  add one, in both themes.
- **The kit.** Build from `web/src/components/ui/` — `Button`, `Badge`, `Dialog`/`Sheet`,
  `Field`/`Input`/`Select`, `Menu`/`Popover`/`Tooltip`, `Tabs`, `Toast`, `Surface`, `Layout`,
  `Icon`. Overlays in particular: the kit's give you a focus trap, Escape and a label; a
  hand-rolled `fixed inset-0` scrim does not.
- **The design scan**, [web/src/lib/design-scan.test.ts](web/src/lib/design-scan.test.ts), reads
  every non-test source file and fails on palette classes, hex outside the code-editor themes and
  agent swatches, white/black outside the kit, and scrims outside the kit and the app shell.
- **The contrast test**, [web/src/lib/contrast.test.ts](web/src/lib/contrast.test.ts), holds
  every text token to WCAG AA (4.5:1) against the surfaces it sits on, in both themes. Changing a
  token's value means this has to stay green.

Check a change in both themes (the switch is in the top bar; the choice is light, dark or
system). A new page goes in `NAV` in `web/src/lib/nav.ts`, which the sidebar, the breadcrumb and
the ⌘K command palette all read. Icons come from `lucide-react` or the kit's hand-drawn `Icon`,
whose `IconName` is a closed union: extend the union and the record together, on its 24 grid,
1.75 stroke, round caps, `currentColor`.

**Pure logic belongs in `web/src/lib/*.ts`**, not in a component. That is the line the vitest
suite is drawn along: anything in `lib` can be tested without a DOM. If you want a test for
something inside a component, move the calculation into `lib` and let the component render its
result.

## Docs move with the code

A change that adds or alters behaviour updates the documents it affects **in the same pull
request**: `README.md`, `docs/architecture.md`, `CLAUDE.md`, `SECURITY.md`, `.env.example`. A
document that lags the code is worse than none, because it is believed.
[crates/eren-cli/src/docs_tests.rs](crates/eren-cli/src/docs_tests.rs) checks what can be
checked:

- every engine id `real_engines()` registers is in README.md and CLAUDE.md;
- every `Capabilities` field is named in CLAUDE.md;
- every `EREN_*` read through `brand::var` / `brand::var_os` is in `.env.example` and README.md;
- every migration number is in `docs/architecture.md`.

It checks that something is said, not that it is right — that part is review.

## Names and compatibility

Eren was called aichip, and that name is written into state that already exists on people's
machines and in their repositories: the home folder, environment variables (Eren reads `AICHIP_*` too),
card branches, committed folders and manifests (Eren still reads `.aichip/` and `aichip.app.yaml`),
the managed database, headers, the app bridge path, browser settings. For Eren every old
spelling lives in three files only:
`crates/eren-shared/src/brand.rs`, `crates/eren-core/src/legacy.rs` and `web/src/lib/brand.ts`.
The rule there is: write the new name, read both, prefer the new.

Everywhere else, ask `brand` for the name: `brand::var("…")` rather than
`std::env::var("EREN_…")`, `brand::home()` rather than joining `.eren` onto the home directory,
`brand::is_card_branch`, `brand::repo_dir`, `brand::app_manifest`, `brand::mcp_tool`, the
header constants, `brand::with_legacy_env` for a child's environment. A name spelled by hand
reads only the new form and strands an existing install. The test `no_old_name_outside_brand`
fails the build when the old name appears anywhere else; in a Markdown file a line may mention
it only if the same line also says Eren.

## Commit conventions

Commits and pull request bodies in this repository carry **no AI attribution of any kind**:

- No `Co-Authored-By` trailer naming a model or tool.
- No "Generated with ..." footer, and no link back to a vendor's site.
- No "AI-assisted", "written by an agent" or equivalent note in the commit body, the PR
  description, or a code comment.

Write the message the way an author writes one: what changed and why, in the imperative. The
existing history is the model to follow. Pull requests use
[.github/pull_request_template.md](.github/pull_request_template.md).

Product-level mentions of Claude Code, OpenCode and the other engines are a different thing
entirely and stay — they are the engines Eren drives, so `ClaudeEngine`, model ids, README text
and UI labels are ordinary code.

## Where things go

**Rust tests live next to the code they test**, in an inline `#[cfg(test)] mod tests` at the
bottom of the file; tests of SQL in an inline `mod db_tests`. There is no `tests/` directory in
any crate and adding one would be the odd one out. Name the test after the behaviour it pins
rather than the function it calls — the existing ones read as sentences
(`an_engine_that_cannot_ask_refuses_reviewed_rather_than_downgrading`), which is what makes a
failure legible in CI output.

**Migrations go in `crates/eren-core/migrations/`**, numbered in sequence (`0089_thing.sql`),
with a line about each in `docs/architecture.md`. They are applied at boot and never edited once
merged. Additive changes are cheap; anything that drops or rewrites data deserves a note in the
file saying why.

Some behaviour is deliberately specified once and shared by both test suites — the expression
language, for instance, exists in `crates/eren-core/src/apps/expr.rs` and
`web/src/lib/expr.ts`, and `crates/eren-core/src/apps/expr_cases.json` is the specification
both read. Add a case to the JSON, not to one side.

## Gotchas that will bite you once

**A new migration may not be picked up.** sqlx embeds `crates/eren-core/migrations/` into the
binary at compile time via `sqlx::migrate!("./migrations")`, and adding a file does not reliably
retrigger a rebuild. The symptom is a new column coming back as `ColumnNotFound` even though the
SQL is obviously right there. The fix:

```bash
touch crates/eren-core/src/db.rs
cargo build
```

**A green `cargo test` is not a running server.** The test run does not replace the binary you
launched, and a server already running keeps the code it started with. To see a change in the
dashboard: `cargo build`, stop `eren serve`, start it again. Front-end changes are different —
`pnpm dev` hot-reloads them, but a change you only made in `web/dist` via `pnpm build` still
needs a page refresh.

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
- comes with tests where the behaviour is testable without a model, which the mock engine and
  stand-in binaries make true of nearly everything;
- updates the docs the change affects;
- passes `cargo fmt`, both test suites, `pnpm exec tsc -b` and `pnpm build`;
- explains its reasoning in comments where the code is doing something non-obvious. The
  existing comments in this codebase state *why*, not *what*, and a patch that follows that
  habit is easier to accept.
