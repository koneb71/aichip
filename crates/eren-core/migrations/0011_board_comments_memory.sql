-- Kanban conversations and agent memory.

-- Comments on a task card — the discussion thread under a card, like Jira.
-- A comment is written by the user or by an agent replying to an @-mention;
-- agent replies carry the run that produced them so cost/transcript are
-- reachable from the thread.
CREATE TABLE task_comments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author TEXT NOT NULL, -- user | agent
    agent_id UUID REFERENCES agents(id) ON DELETE SET NULL,
    content TEXT NOT NULL,
    run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX task_comments_task ON task_comments (task_id, created_at);

-- What an agent remembers. Written automatically when an agent finishes a
-- task or answers a mention; injected into that agent's next runs so it knows
-- what it has been working on. Rows are small on purpose — this is a memory,
-- not a transcript archive.
CREATE TABLE agent_memories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE CASCADE,
    task_id UUID REFERENCES tasks(id) ON DELETE SET NULL,
    kind TEXT NOT NULL DEFAULT 'note', -- task_result | comment_reply | note
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX agent_memories_agent ON agent_memories (agent_id, created_at DESC);

-- Manual card ordering within a column (drag and drop). Seconds-since-epoch
-- as the default keeps insertion order without a separate sequence; a dragged
-- card takes the midpoint of its new neighbours.
ALTER TABLE tasks ADD COLUMN position DOUBLE PRECISION NOT NULL
    DEFAULT extract(epoch FROM now());
UPDATE tasks SET position = extract(epoch FROM created_at);

-- A run that answers an @-mention. Kept off tasks.task_id so the dispatcher
-- can tell a reply run from a coding run on the same card.
ALTER TABLE runs ADD COLUMN comment_id UUID REFERENCES task_comments(id) ON DELETE CASCADE;
