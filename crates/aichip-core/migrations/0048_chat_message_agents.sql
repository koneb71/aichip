-- Which agents a chat message mentioned by name.
--
-- A join table rather than a `UUID[]` on the message, for one reason: an agent
-- can be deleted. A dangling id in an array would be discovered later, when
-- `create_task` tried to bind it and hit the foreign key on `tasks.agent_id` --
-- i.e. as a failed run. Here the cascade removes the mention when the agent
-- goes, and the turn simply behaves as though nobody was mentioned.
CREATE TABLE chat_message_agents (
    message_id UUID NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    agent_id   UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    -- Where in the message the mention appeared. The first one is what a turn
    -- that creates a single task binds to, so the order has to survive.
    position   INT NOT NULL,
    PRIMARY KEY (message_id, agent_id)
);

CREATE INDEX chat_message_agents_message ON chat_message_agents (message_id, position);
