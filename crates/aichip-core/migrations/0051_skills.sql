-- Skills: a named way of doing something, smaller than an agent.
--
-- The gap, counted: all 12 agents in this workspace carry 500–900 character
-- system prompts, because an agent was the only reusable instruction there
-- was. So "do this task, following our release checklist" meant inventing a
-- whole persona for the checklist. An agent is *who* does the work; a skill is
-- *how* a particular job is done, and the two compose.
--
-- Workspace-scoped like agents, and for the same reason: how you do a thing
-- outlives the repository you first did it in.
CREATE TABLE skills (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    -- When to reach for it. Shown in the picker, and the only thing a person
    -- reads before choosing.
    description  TEXT NOT NULL DEFAULT '',
    -- How to do it.
    instructions TEXT NOT NULL DEFAULT '',
    -- What it must not do. Its own column rather than a paragraph inside the
    -- instructions, because "explicit about what it should not do" is the part
    -- free text always omits — a field asks the question.
    must_not     TEXT NOT NULL DEFAULT '',
    -- Off, not deleted. The remedy for a skill that is steering runs wrongly is
    -- to disable it and retry a plain run; deleting to test would destroy the
    -- thing being tested.
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One `@` namespace with agents, so `@Frontend` and `@release-checklist` read
-- the same way and share one picker. Uniqueness within skills is enforced here;
-- the cross-table half — a skill may not take an agent's name, or the reverse —
-- is checked in `skills::check_name_free`, because a constraint cannot span two
-- tables and a trigger would hide the rule from the code that has to explain it.
CREATE UNIQUE INDEX skills_ws_name ON skills (workspace_id, lower(name));

-- Which skill a card was created with. SET NULL rather than CASCADE: deleting
-- the way you used to do something must not delete the work that was done.
ALTER TABLE tasks ADD COLUMN skill_id UUID REFERENCES skills(id) ON DELETE SET NULL;

-- Which skills a chat message named, mirroring `chat_message_agents`. Same
-- reason it is a table rather than an array: a skill can be deleted, and a
-- dangling id would surface later as a failed run rather than as a mention
-- that quietly stopped applying.
CREATE TABLE chat_message_skills (
    message_id UUID NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    skill_id   UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    position   INT NOT NULL,
    PRIMARY KEY (message_id, skill_id)
);

CREATE INDEX chat_message_skills_message ON chat_message_skills (message_id, position);
