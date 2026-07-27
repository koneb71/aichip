-- A board task can be handed to a whole team instead of one agent. The team
-- run keeps the task's worktree, so review and merge work exactly as they do
-- for a solo task.
ALTER TABLE tasks ADD COLUMN team_id UUID REFERENCES teams(id) ON DELETE SET NULL;
