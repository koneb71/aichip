-- Handing a running card to another agent, with a note.
--
-- The request is recorded on the card, the running agent is stopped, and
-- once nothing of the card is executing any more the new agent picks the
-- work up in the same worktree — the note is its brief. Recorded first so
-- a restart in between still completes it (the scheduler sweeps these).
ALTER TABLE tasks
    ADD COLUMN handoff_agent_id     UUID REFERENCES agents(id) ON DELETE SET NULL,
    ADD COLUMN handoff_note         TEXT,
    ADD COLUMN handoff_from_run_id  UUID REFERENCES runs(id) ON DELETE SET NULL,
    ADD COLUMN handoff_requested_at TIMESTAMPTZ;

-- The run a stopped run was handed on to, for the card's history.
ALTER TABLE runs ADD COLUMN handed_to_run_id UUID REFERENCES runs(id) ON DELETE SET NULL;

CREATE INDEX tasks_handoff_pending ON tasks (handoff_requested_at) WHERE handoff_requested_at IS NOT NULL;
