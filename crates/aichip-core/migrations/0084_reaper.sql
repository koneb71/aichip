-- Runs that stop showing signs of life.
--
-- `last_event_at` is when the run last said anything, written at most every
-- fifteen seconds — for the dashboard; the reaper itself reads the in-memory
-- registry, which no row can stand in for.
--
-- `reaped` says why aichip stopped a run itself: 'lost' (the process working
-- on it is gone) or 'silent' (no output for longer than the person allowed).
-- `auto_resumed_at` is set once aichip has tried to pick it back up, so a
-- refused resume is not retried every tick.
ALTER TABLE runs
    ADD COLUMN last_event_at   TIMESTAMPTZ,
    ADD COLUMN reaped          TEXT CHECK (reaped IN ('lost', 'silent')),
    ADD COLUMN auto_resumed_at TIMESTAMPTZ;
