-- Let an agent defer to the workspace default instead of dictating its own
-- permission mode.
--
-- `execute_task_run` resolves the mode as `agent_preset ?? task.permission_mode`,
-- so a bound agent's preset overrode everything — including the workspace
-- default. Since most cards have an agent, setting "don't ask" globally
-- changed nothing: the agent still said `auto_edit`, which auto-approves file
-- edits but stops for every shell command. That is the "Allow Bash?" prompt
-- that kept appearing after the default was already switched off.
--
-- NULL now means "inherit", which is the right default for a new agent: an
-- agent describes *what someone is good at*, not how much you trust the
-- machine that day. Existing rows keep their explicit value, because silently
-- widening what an already-configured agent may do would be the wrong kind of
-- helpful.
ALTER TABLE agents ALTER COLUMN permission_preset DROP NOT NULL;
ALTER TABLE agents ALTER COLUMN permission_preset DROP DEFAULT;
