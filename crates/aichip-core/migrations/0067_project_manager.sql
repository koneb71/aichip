-- A project manager: an agent that reviews the board on a schedule and acts
-- on it, with nobody watching.
--
-- Deliberately not a new executor. A project chat already holds exactly the
-- toolbox a manager needs — list_tasks, get_task_status, get_diff, get_spend,
-- create_task, start_task, move_task, cancel_task — and a routine already
-- fires a chat turn into a standing thread on a cron, where session resume is
-- what lets "since yesterday" mean anything. So a manager is a fifth routine
-- kind ('manage') wearing a composed prompt, and what is new here is the two
-- things that were genuinely missing: naming the agent, and bounding what an
-- unattended pass may spend.
--
-- The agent as a column, not an @mention in the prompt text. A routine prompt
-- already resolves mentions, but a manager is *assigned*, and an assignment
-- that lives inside prose can be edited away by accident and cannot be shown
-- as a chip on the project.
ALTER TABLE routines ADD COLUMN agent_id UUID REFERENCES agents(id) ON DELETE SET NULL;

-- How many cards one pass may put into flight. NULL for every other kind —
-- a chat or a watch starts nothing. The rail exists because the failure mode
-- is specific and expensive: one confused pass at 3am starting nine cards at
-- a couple of dollars each, against a subscription the user pays for. A cap
-- turns "it went wrong" into "it went wrong twice".
ALTER TABLE routines ADD COLUMN max_starts INT;

-- At most one manager per project. A partial unique index rather than a
-- column on `projects`, so the schedule, the engine, the tier, the effort,
-- the catch-up policy and the firing history are the ones routines already
-- has, instead of a second copy that drifts.
CREATE UNIQUE INDEX routines_one_manager_per_project
    ON routines (project_id) WHERE kind = 'manage';

-- …and the index alone does not say that, because `project_id` is nullable
-- and Postgres treats NULLs as distinct: without this, any number of
-- project-less managers satisfy the unique index above. A manager with no
-- board is not a degraded manager, it is a firing that can only fail, so it
-- is refused by the schema rather than caught at 9am.
ALTER TABLE routines ADD CONSTRAINT routines_manager_needs_a_board
    CHECK (kind <> 'manage' OR project_id IS NOT NULL);

-- What a pass actually did.
--
-- Two jobs in one table, and they are the same job. It is the cap's counter —
-- `max_starts` is enforced by counting rows here for the pass in flight, not
-- by trusting the model to count its own tool calls. And it is the record a
-- person reads in the morning: an unsupervised feature that cannot say what
-- it did overnight is one you turn off after the first surprise.
--
-- Keyed to `routine_runs`, which is already the row for one firing, so a pass
-- that produced nothing still has its history. CASCADE from there: the log of
-- what a firing did is meaningless once the firing itself is gone. `task_id`
-- is SET NULL for the opposite reason — the manager started a card, and
-- deleting the card later does not unmake that.
CREATE TABLE manager_actions (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    routine_run_id UUID NOT NULL REFERENCES routine_runs(id) ON DELETE CASCADE,
    -- 'create' | 'start' | 'move' | 'cancel'. Text rather than an enum, the
    -- same choice `routine_runs.trigger` makes: a sixth acting tool should be
    -- a line in the recorder, not a migration.
    kind           TEXT NOT NULL,
    task_id        UUID REFERENCES tasks(id) ON DELETE SET NULL,
    -- The card's title as it was at the time, so the log still reads after
    -- the card is renamed or deleted, plus whatever the action needs to be
    -- legible ("backlog → review").
    detail         TEXT NOT NULL DEFAULT '',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX manager_actions_pass ON manager_actions (routine_run_id, created_at);
