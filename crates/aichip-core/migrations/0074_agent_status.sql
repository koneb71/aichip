-- Whether an agent takes work.
--
--   active            takes work
--   paused            keeps its cards and history, starts nothing until resumed
--   retired           takes no new work, hidden from pickers; its history stays
--   pending_approval  reserved for an agent proposed by another agent: every
--                     door already refuses it, so that flow can be added later
--                     without a second pass over the doors
ALTER TABLE agents ADD COLUMN status TEXT NOT NULL DEFAULT 'active'
    CHECK (status IN ('active', 'paused', 'retired', 'pending_approval'));
ALTER TABLE agents ADD COLUMN pause_reason TEXT;
ALTER TABLE agents ADD COLUMN paused_at TIMESTAMPTZ;
