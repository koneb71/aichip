ALTER TABLE workflows ADD COLUMN last_run_at TIMESTAMPTZ;
ALTER TABLE workflows ADD COLUMN description TEXT NOT NULL DEFAULT '';

-- Steps of a workflow run are displayed in creation order.
CREATE INDEX steps_run ON steps (run_id, started_at);
