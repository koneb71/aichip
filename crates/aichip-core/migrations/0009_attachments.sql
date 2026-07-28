-- Files a user attaches to a task prompt or a chat message.
--
-- The bytes live outside every git tree, under ~/.aichip/attachments/<id>/,
-- and are named to the model by absolute path. Putting them in the task
-- worktree instead would make them untracked files in `git status`, and any
-- agent that runs `git add -A` would commit the user's PDF to their branch
-- and then to main on squash-merge. Chat runs are worse still: their cwd is
-- the user's real checkout.
--
-- Uploads happen before the task/message row exists, so an attachment is born
-- owned only by its project and is "claimed" by the create/send call.
CREATE TABLE attachments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    task_id UUID REFERENCES tasks(id) ON DELETE CASCADE,
    message_id UUID REFERENCES chat_messages(id) ON DELETE CASCADE,
    -- Sanitized basename: safe to join into a path and to echo back to the UI.
    filename TEXT NOT NULL,
    -- Derived from the extension server-side. The browser-supplied part
    -- Content-Type is never trusted.
    mime TEXT NOT NULL,
    -- image | pdf | text — drives prompt wording and Content-Disposition.
    kind TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    -- Absolute host path, stored rather than recomputed so that changing the
    -- storage root later cannot orphan existing rows.
    disk_path TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- An attachment belongs to a task or a message, never both.
    CONSTRAINT attachments_single_owner CHECK (task_id IS NULL OR message_id IS NULL)
);

CREATE INDEX attachments_task_idx ON attachments (task_id) WHERE task_id IS NOT NULL;
CREATE INDEX attachments_message_idx ON attachments (message_id) WHERE message_id IS NOT NULL;
-- Drives the sweeper: uploads abandoned by closing the composer.
CREATE INDEX attachments_unclaimed_idx ON attachments (created_at)
    WHERE task_id IS NULL AND message_id IS NULL;
