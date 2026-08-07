-- Per-project defaults for what a card runs on.
--
-- `full_auto_opt_in` was the only per-project preference that existed, so
-- "this project always runs on OpenCode" or "complex here means the big model"
-- had to be set card by card, every time. NULL means inherit — the same
-- convention `chats.model_tier` and `agents.effort` already use, and the reason
-- these are three nullable columns rather than a JSON blob: a NULL is a fact
-- the resolver already knows how to read.
--
-- Deliberately not constrained to a fixed list of engine ids. The engine
-- registry is discovered at boot from what is on PATH, so a CHECK here would be
-- a second, staler copy of it — and a project pinned to an engine the user
-- later uninstalls should degrade to the default, which is what the resolver
-- does, not fail its migration.
ALTER TABLE projects ADD COLUMN default_engine TEXT;
ALTER TABLE projects ADD COLUMN default_tier   TEXT;
ALTER TABLE projects ADD COLUMN default_effort TEXT;
