-- The pull request a card was finished as.
--
-- Six columns rather than a `pull_requests` table: a task has at most one pull
-- request, forever, and there is no intermediate lifecycle to record. Previews
-- earned a table because they have building/running/stopped, a container id, a
-- port and a sweeper; this has none of that.
--
-- Everything here is a **cache of what `gh` said**, which is why the last
-- column exists.
ALTER TABLE tasks ADD COLUMN pr_url TEXT;

-- Addressing that survives the branch.
--
-- `gh pr view <branch>` stops resolving the moment somebody deletes the branch
-- after merging, which is the ordinary tidy-up. `gh pr view <number>` keeps
-- working forever, so without this a merged-and-tidied card would lose its
-- pull request permanently.
ALTER TABLE tasks ADD COLUMN pr_number INTEGER;

-- open | draft | merged | closed, in aichip's words rather than gh's.
ALTER TABLE tasks ADD COLUMN pr_state TEXT;

-- none | pending | passing | failing — the roll-up of every check, computed
-- fail-closed so a state nobody has seen before never reads as green.
ALTER TABLE tasks ADD COLUMN pr_checks TEXT;

-- approved | changes_requested | review_required, or NULL when GitHub said
-- nothing (a repository with no review rules says nothing, and that is not the
-- same as "not yet approved").
ALTER TABLE tasks ADD COLUMN pr_review TEXT;

-- When the five above were last true.
--
-- The column that makes the other five honest. Without it the UI cannot tell
-- "checks are passing" from "checks were passing an hour ago", and a cache
-- shown as though it were live is worse than no cache. Same choice as
-- `previews.stale`: show the age, never imply freshness.
ALTER TABLE tasks ADD COLUMN pr_synced_at TIMESTAMPTZ;

-- The board list filters on this to draw its chip, and the epic roll-up
-- reads the same rows.
CREATE INDEX IF NOT EXISTS tasks_with_pull_requests
    ON tasks (pr_number)
    WHERE pr_number IS NOT NULL;
