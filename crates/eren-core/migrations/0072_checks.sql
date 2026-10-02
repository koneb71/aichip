-- Checks: the project's own test and lint commands, run on an agent's work
-- before a person reviews it.
--
-- Until now a run finished, the card went to Review, and the first anyone
-- learned the tests were red was by reading the diff and running them by hand.
--
-- `project_checks` holds shell commands this machine will run, so it is a
-- side table with exactly one writer — `routes/checks.rs`, behind the same
-- write header as the attention hook — and a test that fails if any other
-- file in the workspace writes to it. A column on `projects` would not do:
-- project rows are written by GitHub import and by agents building apps, and
-- `attention.rs` explains why a shell command must never sit where an agent
-- can write it.
CREATE TABLE project_checks (
    project_id        UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    -- [{ "name": "tests", "command": "cargo test" }, …], run in order.
    commands          JSONB NOT NULL DEFAULT '[]',
    -- Per command. A hung test suite must not hold the queue forever.
    timeout_secs      INT NOT NULL DEFAULT 600,
    -- How many "fix failing checks" runs may start by themselves before it
    -- stops for a person. Only ever acted on for Full Auto runs.
    auto_fix_attempts INT NOT NULL DEFAULT 0,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One execution of a project's checks against one card's worktree.
CREATE TABLE check_runs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id     UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- The agent run whose work was checked. NULL once that run is deleted;
    -- the result still describes the worktree as it stood.
    run_id      UUID REFERENCES runs(id) ON DELETE SET NULL,
    -- queued | running | passed | failed | error | canceled
    status      TEXT NOT NULL DEFAULT 'queued',
    -- 'auto' (a Full Auto run finishing) or 'person' (someone clicked).
    started_by  TEXT NOT NULL,
    -- [{ name, command, exitCode, timedOut, ms, outputTail }], in order.
    results     JSONB NOT NULL DEFAULT '[]',
    -- Paths the checks themselves left changed in the worktree — build output
    -- a later commit would otherwise sweep into the card's diff.
    dirtied     JSONB NOT NULL DEFAULT '[]',
    error       TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);
CREATE INDEX check_runs_task ON check_runs (task_id, created_at DESC);
