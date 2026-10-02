-- Routines: a prompt that runs on a schedule.
--
-- Three kinds, each landing where that work naturally lives — a chat turn in
-- a standing thread, a fresh research report, or a board card started on its
-- project. The routine row is the schedule and the prompt; what a firing
-- produced is a routine_runs row pointing at the ordinary artifact, so
-- everything downstream (activity, spend, notifications) already works.
CREATE TABLE routines (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    -- 'chat' | 'research' | 'task'
    kind         TEXT NOT NULL,
    -- NULL = general (chat and research only; a task always needs a board).
    project_id   UUID REFERENCES projects(id) ON DELETE CASCADE,
    prompt       TEXT NOT NULL,
    -- Five-field cron, evaluated in the server's local time: a person writing
    -- "0 9 * * *" means their own 9am, not UTC's.
    cron_expr    TEXT NOT NULL,
    -- 'run_once' (default) or 'skip'. This machine is a laptop: the 9am
    -- routine must survive the lid being closed at 9am, so a missed window
    -- catches up once on wake unless the user opts out.
    catch_up     TEXT NOT NULL DEFAULT 'run_once',
    enabled      BOOLEAN NOT NULL DEFAULT true,
    -- NULL = the default engine / the kind's own defaults, resolved at fire
    -- time rather than frozen at save time.
    engine       TEXT,
    model_tier   TEXT,
    effort       TEXT,
    -- The chat kind's standing thread, created on first fire so an unused
    -- routine leaves no empty chat behind. SET NULL: deleting the thread
    -- re-arms creation instead of killing the routine.
    chat_id      UUID REFERENCES chats(id) ON DELETE SET NULL,
    -- The scheduler's bookmark, same semantics as workflows.last_fired_at.
    last_fired_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX routines_workspace ON routines (workspace_id, created_at DESC);

-- One row per firing — the routine's history. The artifact links are SET
-- NULL, not CASCADE: deleting a report or a card should leave the fact that
-- the routine fired, or the history lies about reliability.
CREATE TABLE routine_runs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    routine_id  UUID NOT NULL REFERENCES routines(id) ON DELETE CASCADE,
    fired_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 'schedule' | 'manual'
    trigger     TEXT NOT NULL,
    run_id      UUID REFERENCES runs(id) ON DELETE SET NULL,
    research_id UUID REFERENCES researches(id) ON DELETE SET NULL,
    task_id     UUID REFERENCES tasks(id) ON DELETE SET NULL,
    chat_id     UUID REFERENCES chats(id) ON DELETE SET NULL,
    -- Why this firing produced nothing (engine missing, thread busy, …).
    -- A firing that failed to enqueue is history too.
    error       TEXT
);
CREATE INDEX routine_runs_routine ON routine_runs (routine_id, fired_at DESC);
