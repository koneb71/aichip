-- Goals: what the work is for.
--
-- A tree per workspace (a company goal, the goals under it, …). Cards and
-- routines point at the goal they serve, and every run of a card is told the
-- chain from the top down — "why this matters" — so an agent decides the
-- small things the way the person would.
--
-- Progress is never stored: it is counted from the cards in a goal's subtree
-- whenever it is asked for, so it cannot drift from the board.
CREATE TABLE goals (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id    UUID REFERENCES goals(id) ON DELETE SET NULL,
    title        TEXT NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 200),
    description  TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 4000),
    status       TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'achieved', 'abandoned')),
    target_date  DATE,
    position     DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (parent_id IS DISTINCT FROM id)
);
CREATE INDEX goals_workspace ON goals (workspace_id, parent_id, position);

ALTER TABLE tasks    ADD COLUMN goal_id UUID REFERENCES goals(id) ON DELETE SET NULL;
ALTER TABLE routines ADD COLUMN goal_id UUID REFERENCES goals(id) ON DELETE SET NULL;
CREATE INDEX tasks_goal ON tasks (goal_id) WHERE goal_id IS NOT NULL;

-- Which projects a goal is pursued in, for the goal's page.
CREATE TABLE project_goals (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    goal_id    UUID NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    PRIMARY KEY (project_id, goal_id)
);

-- A card split out of an epic serves the epic's goal unless it names its
-- own. A trigger rather than a rule at each insert: epics are split in more
-- than one place, and the one that forgot would quietly make goalless work.
CREATE FUNCTION tasks_inherit_goal() RETURNS trigger AS $$
BEGIN
    IF NEW.goal_id IS NULL AND NEW.parent_id IS NOT NULL THEN
        SELECT goal_id INTO NEW.goal_id FROM tasks WHERE id = NEW.parent_id;
    END IF;
    RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER tasks_inherit_goal BEFORE INSERT ON tasks
    FOR EACH ROW EXECUTE FUNCTION tasks_inherit_goal();
