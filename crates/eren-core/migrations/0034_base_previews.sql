-- Preview the branch a card will merge into, so "does this look right" has
-- something to be right *against*.
--
-- This is the slice that was going to be "standing per-project deployments"
-- until it became clear the user already solves that themselves — every
-- project here already picks its own ports with ${FRONTEND_PORT:-9000}, and
-- duplicating that would be a worse version of something that works.
--
-- What they cannot do today is see this card and main side by side. Same
-- machinery, one nullable column: a preview with no task is the project's own
-- checkout rather than a card's worktree.
ALTER TABLE previews ALTER COLUMN task_id DROP NOT NULL;

-- The existing index covers per-card previews and skips these, because a NULL
-- is never equal to anything in a unique index. So the project-level one needs
-- its own, or a project could accumulate a base preview per click.
CREATE UNIQUE INDEX IF NOT EXISTS previews_one_alive_base_per_project
    ON previews (project_id)
    WHERE task_id IS NULL AND status IN ('building', 'running', 'idle');
