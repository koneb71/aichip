-- The board's first hierarchy: an epic and its tickets.
--
-- A manager's plan already decomposes a goal into briefed, owned, ordered units
-- of work — but they lived only as `steps` rows, which are visible in the team
-- room, invisible on the board, and deleted with the run. One card went in, one
-- card came out, and the shape of the work in between left no trace.
--
-- SET NULL rather than CASCADE, and the difference is the point: a sub-ticket is
-- real work with its own comments, run history and diff. Deleting the epic must
-- orphan its tickets, never destroy them.
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS parent_id UUID REFERENCES tasks(id) ON DELETE SET NULL;
ALTER TABLE tasks ADD CONSTRAINT tasks_parent_not_self CHECK (parent_id IS DISTINCT FROM id);
CREATE INDEX IF NOT EXISTS tasks_parent_idx ON tasks (parent_id) WHERE parent_id IS NOT NULL;

-- Which card an assignment became.
ALTER TABLE steps ADD COLUMN IF NOT EXISTS task_id UUID REFERENCES tasks(id) ON DELETE SET NULL;

-- And the tombstone, which is load-bearing rather than bookkeeping.
--
-- `task_id IS NULL` cannot mean "needs a card", because deleting a card sets it
-- back to NULL: the next reconcile would create the card again, the user would
-- delete it again, forever. NULL `task_id` means "no card right now"; NULL
-- `task_linked_at` means "never had one". Only the second is an instruction.
ALTER TABLE steps ADD COLUMN IF NOT EXISTS task_linked_at TIMESTAMPTZ;

-- One card belongs to at most one assignment.
CREATE UNIQUE INDEX IF NOT EXISTS steps_task_idx ON steps (task_id) WHERE task_id IS NOT NULL;
