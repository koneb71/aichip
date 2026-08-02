-- DDL an app's manifest implies, waiting for someone to read it.
--
-- There is deliberately **no** table here mirroring what an app's schema
-- currently looks like. `information_schema` already knows, and it is the only
-- answer that cannot be wrong: a registry drifts the first time anything
-- touches a table outside the code that maintains the registry, and then the
-- diff is computed against a fiction. So the live side of every comparison is
-- read from Postgres, and this table holds only the other side — what has been
-- proposed and not yet run.
--
-- Additive statements never land here. Creating a table, adding a nullable
-- column or adding an index cannot lose anything, so asking about them would
-- be a dialog that always gets the same answer, and a dialog that always gets
-- the same answer trains people to stop reading it.
--
-- What does land here is everything that destroys: a dropped column, a dropped
-- table, a changed type. An agent rewriting a manifest must never silently drop
-- a column of someone's data, and the honest way to prevent that is to show the
-- literal SQL and wait — the same shape as an unapproved preview recipe, and
-- for the same reason.
CREATE TABLE IF NOT EXISTS app_schema_plans (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    app_id      UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE,

    -- Every statement in the plan, additive ones included, each with the
    -- sentence explaining what it does. The whole plan is stored rather than
    -- only the destructive part, because approving half a migration and
    -- inferring the rest at apply time would mean running SQL nobody saw.
    statements  JSONB NOT NULL,

    -- Whether any statement in it destroys something. The column exists so the
    -- question "does this need a person?" is answered by a read rather than by
    -- re-deriving it from the JSON at every call site.
    destructive BOOLEAN NOT NULL DEFAULT FALSE,

    status      TEXT NOT NULL DEFAULT 'pending',
    error       TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    applied_at  TIMESTAMPTZ,

    CONSTRAINT app_schema_plans_status_known
        CHECK (status IN ('pending', 'applied', 'discarded', 'failed'))
);

-- One outstanding question per app. Two pending plans would mean approving the
-- older one applies a migration computed against a manifest that has since
-- changed — so a new proposal replaces the old rather than queueing behind it.
CREATE UNIQUE INDEX IF NOT EXISTS app_schema_plans_one_pending
    ON app_schema_plans (app_id)
    WHERE status = 'pending';
