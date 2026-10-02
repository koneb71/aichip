-- What an agent did on each heartbeat. One row per beat, kept to the last
-- 500 per agent: enough to read a week of a 15-minute heartbeat, and the
-- answer to "is my agent actually picking up work, or just idling?".
CREATE TABLE heartbeats (
    id       BIGSERIAL PRIMARY KEY,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    reason   TEXT NOT NULL CHECK (reason IN ('timer', 'wake')),
    outcome  TEXT NOT NULL CHECK (outcome IN ('started', 'fired', 'idle', 'busy', 'held', 'paused')),
    task_id  UUID REFERENCES tasks(id) ON DELETE SET NULL,
    run_id   UUID REFERENCES runs(id) ON DELETE SET NULL,
    detail   TEXT NOT NULL DEFAULT ''
);
CREATE INDEX heartbeats_agent ON heartbeats (agent_id, at DESC);
