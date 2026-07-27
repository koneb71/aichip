-- Organizations: a team with a manager who plans the work, delegates it to
-- specialists, and reviews the result. An org run is one `run` whose steps
-- are the manager's assignments.
ALTER TABLE runs ADD COLUMN team_id UUID REFERENCES teams(id) ON DELETE SET NULL;
ALTER TABLE runs ADD COLUMN goal TEXT;
-- Task/workflow/chat runs reach their project through a join; an org run
-- belongs to a project directly.
ALTER TABLE runs ADD COLUMN project_id UUID REFERENCES projects(id) ON DELETE CASCADE;

-- Steps double as assignments once a manager has planned them.
ALTER TABLE steps ADD COLUMN assignee TEXT;
ALTER TABLE steps ADD COLUMN title TEXT;
ALTER TABLE steps ADD COLUMN brief TEXT;
ALTER TABLE steps ADD COLUMN depends_on TEXT[] NOT NULL DEFAULT '{}';

-- What the team says to each other. This is the feed the user watches.
CREATE TABLE org_messages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    step_id UUID REFERENCES steps(id) ON DELETE SET NULL,
    seq BIGSERIAL,
    from_agent TEXT NOT NULL,
    to_agent TEXT,                     -- NULL = addressed to the whole team
    kind TEXT NOT NULL,                -- assignment | message | question | answer | status | result
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX org_messages_run ON org_messages (run_id, seq);
