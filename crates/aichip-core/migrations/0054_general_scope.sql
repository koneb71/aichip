-- General chats and researches: not connected to any project.
--
-- A conversation does not always have a repository behind it, and neither
-- does a question worth researching. Both rows gain a workspace to belong to
-- instead — the workspace is what scopes agents, skills and the knowledge
-- base, all of which a project-less chat or research still uses.
--
-- Backfilled from the owning project so every existing row carries both, and
-- CHECKed so a row can never carry neither: a chat that belongs to nothing
-- would be listed nowhere and resolve mentions against nothing.
ALTER TABLE chats ALTER COLUMN project_id DROP NOT NULL;
ALTER TABLE chats ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;
UPDATE chats c SET workspace_id = p.workspace_id
  FROM projects p WHERE p.id = c.project_id;
ALTER TABLE chats ADD CONSTRAINT chats_belong_somewhere
  CHECK (project_id IS NOT NULL OR workspace_id IS NOT NULL);
CREATE INDEX chats_general ON chats (workspace_id, updated_at DESC)
  WHERE project_id IS NULL;

ALTER TABLE researches ALTER COLUMN project_id DROP NOT NULL;
ALTER TABLE researches ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;
UPDATE researches r SET workspace_id = p.workspace_id
  FROM projects p WHERE p.id = r.project_id;
ALTER TABLE researches ADD CONSTRAINT researches_belong_somewhere
  CHECK (project_id IS NOT NULL OR workspace_id IS NOT NULL);
CREATE INDEX researches_general ON researches (workspace_id, created_at DESC)
  WHERE project_id IS NULL;
