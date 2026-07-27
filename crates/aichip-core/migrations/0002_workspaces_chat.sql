CREATE TABLE workspaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    icon TEXT NOT NULL DEFAULT 'sparkles',
    color TEXT NOT NULL DEFAULT '#4f46e5',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO workspaces (name) VALUES ('My Workspace');

ALTER TABLE projects ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;
UPDATE projects SET workspace_id = (SELECT id FROM workspaces LIMIT 1);
ALTER TABLE projects ALTER COLUMN workspace_id SET NOT NULL;

ALTER TABLE agents ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;
UPDATE agents SET workspace_id = (SELECT id FROM workspaces LIMIT 1);
ALTER TABLE agents ALTER COLUMN workspace_id SET NOT NULL;

ALTER TABLE teams ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;
UPDATE teams SET workspace_id = (SELECT id FROM workspaces LIMIT 1);
ALTER TABLE teams ALTER COLUMN workspace_id SET NOT NULL;

-- name uniqueness becomes per-workspace
ALTER TABLE agents DROP CONSTRAINT agents_name_key;
ALTER TABLE agents ADD CONSTRAINT agents_ws_name UNIQUE (workspace_id, name);
ALTER TABLE teams DROP CONSTRAINT teams_name_key;
ALTER TABLE teams ADD CONSTRAINT teams_ws_name UNIQUE (workspace_id, name);

CREATE TABLE chats (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT 'Chat',
    session_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE chat_messages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    role TEXT NOT NULL, -- user | assistant | system
    content TEXT NOT NULL,
    run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX chat_messages_chat ON chat_messages (chat_id, created_at);

ALTER TABLE runs ADD COLUMN chat_id UUID REFERENCES chats(id) ON DELETE CASCADE;
ALTER TABLE tasks ADD COLUMN chat_id UUID REFERENCES chats(id) ON DELETE SET NULL;
