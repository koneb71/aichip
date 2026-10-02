-- Apps you install, switch on, and use — without leaving the dashboard.
--
-- An app is a *project*, on disk under ~/.aichip/apps/<slug>, and that is the
-- whole reason this is a small feature rather than a large one. Worktrees,
-- diffs, the files editor, chat, previews and the run orchestrator all key on a
-- project, all had to exist anyway, and none of them should be written a second
-- time under a different name. "Change this app" is a card.
--
-- `kind` defaults to 'repo' so every existing row keeps its meaning with no
-- backfill, and the places that *list* projects filter on it — a gallery of
-- twelve apps must not bury the three repositories someone actually works in.
-- The spend and activity joins deliberately do not filter: generating an app
-- costs real money and belongs on the activity page under the app's name.
ALTER TABLE projects ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'repo';

CREATE TABLE IF NOT EXISTS apps (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- One project, one app. The project is where the manifest lives, what git
    -- tracks, and what a generation run gets a worktree of.
    project_id    UUID NOT NULL UNIQUE REFERENCES projects(id) ON DELETE CASCADE,
    workspace_id  UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,

    -- UNIQUE, unlike `previews.slug`, and the difference is the point. A
    -- preview slug is a *link*: the proxy takes the newest row and a stale one
    -- is a dead tab. An app slug is an *identity* — it is how a request proves
    -- which app it is, and therefore which grants and which tables it gets.
    -- Two rows answering to one name would be two apps sharing a capability.
    slug          TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    icon          TEXT NOT NULL DEFAULT '▦',
    summary       TEXT NOT NULL DEFAULT '',
    -- What the person asked for, kept verbatim. The manifest is what the agent
    -- produced; this is what it was answering, and the two diverge.
    brief         TEXT NOT NULL DEFAULT '',

    -- 'module' renders in aichip and executes nothing. 'node'/'static' are
    -- containers, and carry the whole apparatus that implies.
    runtime       TEXT NOT NULL DEFAULT 'module',

    -- Two states, not three. "Installed but never switched on" and "switched
    -- off again" are the same thing to the person looking at the switch, and a
    -- state nobody can tell apart is a state that gets set wrong. Uninstall is
    -- not a state: it is a row that no longer exists.
    state         TEXT NOT NULL DEFAULT 'active',

    -- The Postgres schema holding this app's tables. Derivable from the slug,
    -- stored anyway: it is the argument to DROP SCHEMA, and a value that
    -- destructive should be read rather than recomputed at the call site.
    schema_name   TEXT NOT NULL,

    -- The live manifest. Also written to the app's folder, where git tracks it
    -- and export reads it — the same both-places write the Files tab already
    -- does. The hash is what says whether they have drifted.
    manifest      TEXT NOT NULL,
    manifest_sha256 TEXT NOT NULL,

    -- What the manifest *asks* for. Never a grant: those live in app_grants,
    -- because a build that starts asking for a new scope has to surface as a
    -- question rather than quietly widening what the app can already do.
    requested_scopes TEXT[] NOT NULL DEFAULT '{}',

    -- Containers only.
    port              INTEGER,
    -- The Dockerfile text a person read, hashed. Not a boolean and not a
    -- timestamp: approval attaches to the text, exactly as approving a preview
    -- recipe does, so the rewrite on build four does not inherit the reading
    -- someone gave build one.
    dockerfile_sha256 TEXT,
    approved_at       TIMESTAMPTZ,

    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT apps_state_known CHECK (state IN ('active', 'inactive')),
    CONSTRAINT apps_runtime_known CHECK (runtime IN ('module', 'node', 'static'))
);

CREATE INDEX IF NOT EXISTS apps_workspace ON apps (workspace_id, created_at DESC);

-- Every attempt to build or change an app, and what it did to the branch.
--
-- `base_commit` is the only column here that is load bearing. An app's build
-- lands on main automatically — reviewing a diff that *is* the whole app is
-- what running it is for, and asking for review on "make the header blue"
-- turns a gallery back into a task board. What makes that bargain honest is
-- that the undo is real, and an undo needs to know where main was before.
CREATE TABLE IF NOT EXISTS app_builds (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    app_id        UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    -- The card that did the work. SET NULL because a build's history should
    -- outlive the card being deleted.
    task_id       UUID REFERENCES tasks(id) ON DELETE SET NULL,
    brief         TEXT NOT NULL,
    base_commit   TEXT,
    landed_commit TEXT,
    status        TEXT NOT NULL DEFAULT 'running',
    error         TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    landed_at     TIMESTAMPTZ,

    CONSTRAINT app_builds_status_known
        CHECK (status IN ('running', 'landed', 'conflicted', 'failed'))
);

CREATE INDEX IF NOT EXISTS app_builds_recent ON app_builds (app_id, created_at DESC);
