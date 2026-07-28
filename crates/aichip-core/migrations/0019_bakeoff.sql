-- Running one task several ways at once, then keeping the best result.
--
-- The activity view can now say what each agent *costs*. It cannot say
-- whether any of them is any good, and picking between a cheap model and an
-- expensive one on vibes is how you end up paying for Opus to rename a
-- variable. A bake-off answers it directly: same brief, two or three
-- attempts, real diffs side by side.
--
-- Most of the machinery already exists. `runs.agent_id` (added for comment
-- replies) and `runs.worktree_path` (added for the org manager) do the work
-- of pointing a run at a particular agent and its own isolated checkout.
-- These two columns are the remainder.

-- Which attempt this is, in the user's language: "Sonnet", "Ada", "high
-- effort". Null for every ordinary run, and its presence is what marks a run
-- as part of a bake-off.
ALTER TABLE runs ADD COLUMN variant_label TEXT;

-- Model tier for this run only, when the variants differ by tier rather than
-- by agent. Null means "whatever the task or its agent already says".
ALTER TABLE runs ADD COLUMN tier_override TEXT;

-- Finding the members of a bake-off means "the variant runs on this task",
-- which is the only query this feature adds to the hot path.
CREATE INDEX runs_variants ON runs (task_id) WHERE variant_label IS NOT NULL;
