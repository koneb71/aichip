-- Indexes for the paths every run walks, and one the busiest path paid twice.
--
-- `events` is written once per stream line of every run. 0001 declared both
-- `UNIQUE (run_id, seq)` and `events_run_seq` on the same two columns; the
-- constraint already builds that exact index, so every event insert was
-- maintaining a second copy of it for nothing.
DROP INDEX IF EXISTS events_run_seq;

-- `events.step_id` is `ON DELETE CASCADE`, and steps are deleted in the
-- ordinary course of things — a plan sent back replaces its 'plan' row. With no
-- index, each of those deletes scanned the whole events table to find the
-- rows to cascade to. Partial, because most events belong to no step.
CREATE INDEX IF NOT EXISTS events_step ON events (step_id) WHERE step_id IS NOT NULL;

-- "Is this card already running?" is asked on every start, board refresh and
-- merge, and a card's runs were only indexed when they were bake-off variants.
CREATE INDEX IF NOT EXISTS runs_task ON runs (task_id);

-- The live runs are a handful and the finished ones are everything else.
-- Boot recovery, the concurrency gate and the board all ask for the handful.
CREATE INDEX IF NOT EXISTS runs_live ON runs (status)
    WHERE status NOT IN ('completed', 'failed', 'canceled');
