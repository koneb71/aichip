CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    default_branch TEXT NOT NULL DEFAULT 'main',
    full_auto_opt_in BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE agents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    icon TEXT NOT NULL DEFAULT 'bot',
    color TEXT NOT NULL DEFAULT '#8b5cf6',
    description TEXT NOT NULL DEFAULT '',
    system_prompt TEXT NOT NULL DEFAULT '',
    model_tier TEXT NOT NULL DEFAULT 'medium',
    allowed_tools TEXT[] NOT NULL DEFAULT '{}',
    permission_preset TEXT NOT NULL DEFAULT 'reviewed',
    engine TEXT NOT NULL DEFAULT 'claude-code',
    builtin BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE teams (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    pattern TEXT NOT NULL, -- pipeline | debate | swarm
    definition JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE tasks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    agent_id UUID REFERENCES agents(id),
    title TEXT NOT NULL,
    prompt TEXT NOT NULL,
    engine TEXT NOT NULL DEFAULT 'claude-code',
    model_tier TEXT NOT NULL DEFAULT 'medium',
    permission_mode TEXT NOT NULL DEFAULT 'reviewed',
    board_column TEXT NOT NULL DEFAULT 'backlog',
    worktree_path TEXT,
    branch TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workflows (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'pipeline',
    source_yaml TEXT NOT NULL DEFAULT '',
    cron_expr TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

CREATE TABLE runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID REFERENCES tasks(id) ON DELETE CASCADE,
    workflow_id UUID REFERENCES workflows(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued',
    trigger TEXT NOT NULL DEFAULT 'manual',
    engine TEXT NOT NULL DEFAULT 'claude-code',
    model TEXT,
    agent_id UUID REFERENCES agents(id),
    session_id TEXT,
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    cost_usd DOUBLE PRECISION,
    input_tokens BIGINT NOT NULL DEFAULT 0,
    output_tokens BIGINT NOT NULL DEFAULT 0,
    error_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE steps (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    step_key TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    session_id TEXT,
    output_text TEXT,
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ
);

CREATE TABLE events (
    id BIGSERIAL PRIMARY KEY,
    run_id UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    step_id UUID REFERENCES steps(id) ON DELETE CASCADE,
    seq BIGINT NOT NULL,
    type TEXT NOT NULL,
    payload JSONB NOT NULL,
    ts TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (run_id, seq)
);
CREATE INDEX events_run_seq ON events (run_id, seq);

CREATE TABLE queue (
    run_id UUID PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
    priority INT NOT NULL DEFAULT 0,
    not_before TIMESTAMPTZ,
    enqueued_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE schedules (
    workflow_id UUID PRIMARY KEY REFERENCES workflows(id) ON DELETE CASCADE,
    cron_expr TEXT NOT NULL,
    catch_up_policy TEXT NOT NULL DEFAULT 'skip',
    last_fired_at TIMESTAMPTZ
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value JSONB NOT NULL
);
