-- How many times this run has been held for a rate limit.
--
-- The ladder in queue/mod.rs was written for this — 5m, then 15m, then 45m —
-- and the only production call site has asked for attempt 0 since the day it
-- landed, so the second and third rungs have only ever run in that file's own
-- tests. An engine that reports no reset time (every OpenCode run) has been
-- retrying every five minutes, forever, against a limit it cannot see.
--
-- On `runs` rather than on `queue`: `claim_next` is a `DELETE … RETURNING
-- run_id`, so a column there would have to be threaded through five signatures
-- to reach the one function that holds. And a resumed or retried run is a new
-- row, so the counter resets without any reset logic.
ALTER TABLE runs ADD COLUMN rate_limit_attempts INT NOT NULL DEFAULT 0;

-- Which run this one is picking up from.
--
-- Resume writes a new row rather than re-queuing the dead one, because
-- `cost_usd` accumulates (re-use would blend a failed attempt's spend into the
-- live one), `events` is unique on (run_id, seq) so a re-used row interleaves
-- RunFailed into the middle of a run that later completes, and `error_reason`
-- and `finished_at` are single-valued — re-queuing means erasing the record of
-- the failure you are resuming from.
ALTER TABLE runs ADD COLUMN resumed_from UUID REFERENCES runs(id) ON DELETE SET NULL;
CREATE INDEX runs_resumed_from ON runs (resumed_from);
