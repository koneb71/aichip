-- The project was renamed from aichip to Eren, and with it the MCP server
-- every engine is told about: its tools were `mcp__aichip__<tool>` and are now
-- `mcp__eren__<tool>`. An agent's allow-list holds those names, so an agent
-- saved before the rename would quietly lose every Eren tool it was allowed.
--
-- Rewritten here rather than only read leniently, so what the agent editor
-- shows is what a run is given. Matched as the whole server (`mcp__aichip`)
-- or a tool of it (`mcp__aichip__…`) — never a different server whose name
-- merely starts the same way. Order is kept.
UPDATE agents
   SET allowed_tools = ARRAY(
           SELECT regexp_replace(t, '^mcp__aichip(__|$)', 'mcp__eren\1')
             FROM unnest(allowed_tools) WITH ORDINALITY AS u(t, n)
            ORDER BY n
       )
 WHERE EXISTS (
           SELECT 1 FROM unnest(allowed_tools) AS t WHERE t ~ '^mcp__aichip(__|$)'
       );
