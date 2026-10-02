-- Things that happened which a project's manager would want to hear about
-- before its next scheduled pass: a card landed, a run failed, a review ran
-- out of rounds. Each is a wake for a routine (or, for heartbeats, an agent).
--
-- Coalesced: the same kind of news about the same card waiting to be read is
-- one row whose count goes up, never a pile — so a card that fails ten times
-- overnight is one line in the morning, not ten passes.
CREATE TABLE wakeups (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    routine_id     UUID REFERENCES routines(id) ON DELETE CASCADE,
    agent_id       UUID REFERENCES agents(id) ON DELETE CASCADE,
    kind           TEXT NOT NULL,
    task_id        UUID REFERENCES tasks(id) ON DELETE CASCADE,
    run_id         UUID REFERENCES runs(id) ON DELETE SET NULL,
    detail         TEXT NOT NULL DEFAULT '',
    count          INT NOT NULL DEFAULT 1,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    consumed_at    TIMESTAMPTZ,
    routine_run_id UUID REFERENCES routine_runs(id) ON DELETE SET NULL,
    CHECK (num_nonnulls(routine_id, agent_id) = 1)
);
-- One open row per (target, kind, card). The zero uuid stands in for "no
-- card", since NULLs never collide in a unique index.
CREATE UNIQUE INDEX wakeups_routine_open
    ON wakeups (routine_id, kind, COALESCE(task_id, '00000000-0000-0000-0000-000000000000'::uuid))
    WHERE consumed_at IS NULL AND routine_id IS NOT NULL;
CREATE UNIQUE INDEX wakeups_agent_open
    ON wakeups (agent_id, kind, COALESCE(task_id, '00000000-0000-0000-0000-000000000000'::uuid))
    WHERE consumed_at IS NULL AND agent_id IS NOT NULL;

-- Which news wakes a manager early, and how often it may be woken. Off
-- (empty) by default: an early pass costs a run, and that is a person's call.
ALTER TABLE routines
    ADD COLUMN on_events          TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN cooldown_secs      INT NOT NULL DEFAULT 900 CHECK (cooldown_secs BETWEEN 60 AND 86400),
    ADD COLUMN max_passes_per_day INT NOT NULL DEFAULT 6 CHECK (max_passes_per_day BETWEEN 1 AND 48);
