-- Scheduling state lives on the workflow itself; `cron_expr` is already
-- derived from the YAML `on.schedule`. The separate `schedules` table from
-- 0001 was never used and would be a second source of truth.
ALTER TABLE workflows ADD COLUMN last_fired_at TIMESTAMPTZ;
ALTER TABLE workflows ADD COLUMN catch_up TEXT NOT NULL DEFAULT 'skip';

DROP TABLE IF EXISTS schedules;
