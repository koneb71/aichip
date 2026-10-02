-- Reporting lines between agents, and the per-agent heartbeat.
--
-- `reports_to` makes the agents of a workspace a tree: a manager agent
-- delegates down it and hears about trouble up it. Kept a tree by the code
-- that writes it (`aichip_core::org_chart`): same workspace, no cycles, a
-- bounded depth, and no retired manager — retiring one lifts its reports to
-- its own manager.
ALTER TABLE agents
    ADD COLUMN reports_to        UUID REFERENCES agents(id) ON DELETE SET NULL,
    ADD COLUMN title             TEXT,
    -- Seconds between heartbeats, NULL for none. A person's standing
    -- decision, like `start_when_unblocked`: see `aichip_core::heartbeat`.
    ADD COLUMN heartbeat_secs    INT CHECK (heartbeat_secs IN (300, 900, 3600, 14400)),
    ADD COLUMN last_heartbeat_at TIMESTAMPTZ;

CREATE INDEX agents_reports_to ON agents (reports_to) WHERE reports_to IS NOT NULL;
