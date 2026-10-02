-- MCP servers the user brings themselves.
--
-- Until now every agent could do exactly three things: read files, write
-- files, run bash. The MCP config aichip generates only ever contained
-- aichip's own endpoint (the permission proxy, or the chat/org tools), so
-- there was no way to hand an agent a browser, a database, or an issue
-- tracker. This is that way.
--
-- Compliance is unchanged and the reason is worth stating: we are still
-- spawning the official CLI and handing it `--mcp-config`, a file the CLI
-- already knows how to read. Nothing here touches credentials, and `env`
-- rejects ANTHROPIC_*/CLAUDE_CODE_OAUTH* keys at the API layer for the same
-- reason the process spawner does.
CREATE TABLE mcp_servers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    -- Becomes the `mcp__<name>__<tool>` prefix the model sees, so it is
    -- slugified on write and unique per workspace.
    name TEXT NOT NULL,
    transport TEXT NOT NULL DEFAULT 'stdio', -- stdio | http | sse
    command TEXT,                            -- stdio: the binary to spawn
    args TEXT[] NOT NULL DEFAULT '{}',
    env JSONB NOT NULL DEFAULT '{}'::jsonb,
    url TEXT,                                -- http/sse: the endpoint
    headers JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name)
);

-- Which agents get which servers.
--
-- Opt-in per agent rather than on for everyone: a "Frontend" agent with
-- write access to the production database is a worse default than a
-- slightly tedious checkbox, and an agent's capabilities should be
-- something you can read off its editor rather than infer from a workspace
-- setting three pages away.
CREATE TABLE agent_mcp_servers (
    agent_id  UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    server_id UUID NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    PRIMARY KEY (agent_id, server_id)
);

CREATE INDEX agent_mcp_servers_by_server ON agent_mcp_servers (server_id);
