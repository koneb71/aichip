-- A conversation with one of the workspace's agents rather than the
-- assistant: its persona, memories, engine, tier and effort, and its budget
-- and limits — the run carries the agent, so every gate that keys on
-- `runs.agent_id` applies. NULL is the assistant, as every chat was before.
--
-- SET NULL rather than CASCADE: deleting an agent should not take the
-- person's half of the conversation with it.
ALTER TABLE chats ADD COLUMN agent_id UUID REFERENCES agents(id) ON DELETE SET NULL;
CREATE INDEX chats_agent ON chats (agent_id) WHERE agent_id IS NOT NULL;
