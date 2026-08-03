-- A build that was undone.
--
-- `0040` wrote the status set before anything read the table, and left out the
-- one state that makes auto-landing defensible. An app's build lands on main
-- without review; the whole bargain is that the undo is real, and an undo that
-- cannot be recorded is one you can perform twice by accident — the second
-- `reset --hard` to the same `base_commit` throwing away a *later* build.
--
-- Kept as a status rather than a `reverted_at` column: a build is in one state,
-- and two columns that must agree eventually disagree.
ALTER TABLE app_builds DROP CONSTRAINT IF EXISTS app_builds_status_known;

ALTER TABLE app_builds ADD CONSTRAINT app_builds_status_known
    CHECK (status IN ('running', 'landed', 'conflicted', 'failed', 'reverted'));
