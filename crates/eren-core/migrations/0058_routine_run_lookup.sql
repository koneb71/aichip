-- Every run that finishes now asks "was this a routine firing?" — one lookup
-- by run_id on a table whose only index was by routine.
CREATE INDEX routine_runs_run ON routine_runs (run_id);
