-- Attachments for chats that belong to no project.
--
-- An attachment belonged to a project because a card or a project chat is
-- what claims it, and the project was the check that an id from one place
-- could not be claimed in another. A general chat has a workspace instead, so
-- an upload now names exactly one of the two, and the claim checks whichever
-- it is. The bytes still live under ~/.eren/attachments, outside every tree.
ALTER TABLE attachments ALTER COLUMN project_id DROP NOT NULL;
ALTER TABLE attachments ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;
ALTER TABLE attachments ADD CONSTRAINT attachments_one_home
    CHECK ((project_id IS NULL) <> (workspace_id IS NULL));
