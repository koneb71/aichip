-- Which files an assignment expects to touch.
--
-- Assignments share one worktree, so two of them running at once is only
-- safe when their file scopes don't overlap — otherwise concurrent writes to
-- the same file silently clobber each other. The manager already names files
-- in its briefs; this makes that structural so the scheduler can check it.
-- Empty means "unknown scope", which is treated as touching everything and
-- therefore runs alone.
ALTER TABLE steps ADD COLUMN touches TEXT[] NOT NULL DEFAULT '{}';
