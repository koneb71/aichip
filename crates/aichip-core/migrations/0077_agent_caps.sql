-- How hard one agent may be worked. All three NULL — no limit — by default.
--
--   max_concurrent  runs at once (its own cards, and its steps of team runs)
--   max_daily_runs  runs started per day
--   cooldown_secs   rest after a run ends before the next one starts
--
-- A run over a limit waits in the queue (queue.hold_reason) rather than
-- failing: these limits shape when work happens, not whether it does.
ALTER TABLE agents ADD COLUMN max_concurrent INT CHECK (max_concurrent > 0);
ALTER TABLE agents ADD COLUMN max_daily_runs INT CHECK (max_daily_runs > 0);
ALTER TABLE agents ADD COLUMN cooldown_secs INT CHECK (cooldown_secs > 0);
