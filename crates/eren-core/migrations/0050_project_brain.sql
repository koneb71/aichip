-- A project's Brain: durable context every run in it starts with.
--
-- The gap this fills, measured before it was written: the knowledge base has
-- worked since it was built, and `task_articles` was empty across 26 cards —
-- nobody had ever attached one. Knowledge that should shape *every* run had to
-- be remembered *every* time, so it was never used at all. `agent_memories`
-- is the other half and is not this: it is written by the agent, scoped to the
-- agent, and is a log of what happened rather than a briefing on how things
-- work here.
--
-- One row per project, not a hierarchy. A wiki already exists for the writing
-- you organise; this is the paragraph you would otherwise repeat in every card.
CREATE TABLE project_brain (
    project_id UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    body       TEXT NOT NULL DEFAULT '',
    -- Disabling rather than deleting is the documented remedy for a brain that
    -- is steering runs wrongly: turn it off, retest a plain run, turn it back
    -- on. Deleting to test would destroy the thing being diagnosed.
    enabled    BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Every save, so "back up before broad rewrites" is automatic rather than
-- advice. Trimmed to the last few on write — this is an undo, not an archive;
-- the wiki is where versioned prose belongs.
CREATE TABLE project_brain_revisions (
    id         BIGSERIAL PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    body       TEXT NOT NULL,
    saved_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX project_brain_revisions_project
    ON project_brain_revisions (project_id, saved_at DESC);
