-- One inbox for everything waiting on a person.
--
-- The inbox itself is a query, not a table: plans, chat questions, schema
-- plans, KB revisions and recipes already have rows that say they are
-- waiting, and an index written beside each of them would be a second record
-- free to disagree with the first. What is here is only what had no row.

-- A permission prompt lived in the broker's memory and nowhere else, so a
-- restart erased the question along with the answer. The full tool input
-- stays in memory; this keeps enough to say what was asked.
CREATE TABLE permission_requests (
    id            TEXT PRIMARY KEY,               -- the broker's request id
    run_id        UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    tool          TEXT NOT NULL,
    input_summary TEXT NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at   TIMESTAMPTZ,
    decision      TEXT CHECK (decision IN ('allowed','denied','unanswered','gone','expired'))
);
CREATE INDEX permission_requests_open ON permission_requests (run_id) WHERE resolved_at IS NULL;
CREATE INDEX permission_requests_expired ON permission_requests (created_at DESC) WHERE decision = 'expired';

-- A card's agent asking a person something. The run finishes its turn rather
-- than holding a slot; the answer comes back as a follow-up in the same
-- worktree with the same session.
CREATE TABLE run_questions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id      UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    task_id     UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    question    TEXT NOT NULL,
    options     JSONB NOT NULL DEFAULT '[]',
    answer      TEXT,
    answered_at TIMESTAMPTZ,
    -- The follow-up the answer started, once it has.
    answer_run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX run_questions_open ON run_questions (task_id) WHERE answered_at IS NULL;

-- Something an agent proposes and only a person can do. The effect is one of
-- a closed set (see aichip_core::decisions::Effect); approving it runs the
-- same function the matching button does.
CREATE TABLE decisions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id   UUID REFERENCES projects(id) ON DELETE CASCADE,
    run_id       UUID REFERENCES runs(id) ON DELETE SET NULL,
    proposed_by  TEXT NOT NULL,
    effect       JSONB NOT NULL,
    reason       TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','approved','denied','failed')),
    outcome      TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at   TIMESTAMPTZ
);
CREATE INDEX decisions_open ON decisions (workspace_id, created_at DESC) WHERE status = 'open';

-- Read and snooze, per inbox item, without touching the item's own row.
CREATE TABLE inbox_marks (
    key           TEXT PRIMARY KEY,
    read_at       TIMESTAMPTZ,
    snoozed_until TIMESTAMPTZ
);
