-- Backfill: step rows left non-terminal under a run that already ended.
--
-- `recover_orphans` and `finish` used to update only the `runs` row, so a
-- crash or restart left steps at 'running' forever. The UI derives "who is
-- working right now" from step status, which is why a failed team run still
-- animated a specialist as "working…". Both writers now settle steps in the
-- same transaction; this cleans up what they left behind.
--
-- Same two buckets as `settle_steps`: mid-flight work really was interrupted,
-- whereas a queued step was never opened and must not read as a failure
-- charged to its assignee.

UPDATE steps s
   SET status = CASE WHEN r.status = 'canceled' THEN 'canceled' ELSE 'failed' END,
       finished_at = COALESCE(s.finished_at, r.finished_at, now())
  FROM runs r
 WHERE s.run_id = r.id
   AND r.status IN ('completed', 'failed', 'canceled')
   AND s.status IN ('starting', 'running', 'waiting_permission', 'rate_limited');

UPDATE steps s
   SET status = 'skipped',
       finished_at = COALESCE(s.finished_at, r.finished_at, now())
  FROM runs r
 WHERE s.run_id = r.id
   AND r.status IN ('completed', 'failed', 'canceled')
   AND s.status = 'queued';
