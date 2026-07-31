-- A Dockerfile an agent wrote, that a person has to read before it is built.
--
-- Kept here rather than written into the branch on purpose. A preview recipe is
-- not part of the change under review — putting it in the worktree would add a
-- file to the diff someone is trying to read, and committing it would put
-- aichip's opinion about how to build the project into the user's repository.
--
-- Per project, not per card: how a project is built does not change from one
-- card to the next, and asking again for every card would be both slow and a
-- new chance to get a different answer.
CREATE TABLE IF NOT EXISTS preview_recipes (
    project_id   UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    dockerfile   TEXT NOT NULL,
    -- 'proposed' until a person has read it. Nothing builds a proposal: a
    -- Dockerfile's RUN lines execute on this machine with the network, so an
    -- unapproved one is agent-authored code waiting to be run.
    status       TEXT NOT NULL DEFAULT 'proposed',
    -- Whether the text is still the agent's, or a person rewrote it.
    edited       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    approved_at  TIMESTAMPTZ
);
