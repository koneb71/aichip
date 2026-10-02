-- When a card's work reached done, and whether the cards it blocks should
-- start by themselves when that happens.
--
-- `landed_at` is the seam every writer of 'done' meets — a merge, a drag, an
-- in-place run, an app build, a PR merged on GitHub, an epic's mirror. It is
-- set once (only where NULL), so however many of them notice the same landing,
-- the cards it unblocks hear about it once. It is cleared when a card leaves
-- done, so landing again is news again.
ALTER TABLE tasks ADD COLUMN landed_at TIMESTAMPTZ;
ALTER TABLE tasks ADD COLUMN start_when_unblocked BOOLEAN NOT NULL DEFAULT FALSE;

-- Cards already done have landed; without this the first sweep after the
-- upgrade would announce every old card's dependents at once.
UPDATE tasks SET landed_at = now() WHERE board_column = 'done';

-- The sweep asks "done but not yet landed" every tick.
CREATE INDEX tasks_landing_pending ON tasks (id)
    WHERE board_column = 'done' AND landed_at IS NULL;
